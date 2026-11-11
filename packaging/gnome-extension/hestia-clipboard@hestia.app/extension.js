// Hestia clipboard: when the clipboard changes, append the text to ~/.local/share/hestia/clipboard.jsonl.
// Settings come from ~/.config/hestia/settings (clipboard=on|off, clipboard_max, clipboard_days).
// Text only. Skipped: empty, huge, and anything a password manager marks with its password hint.
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {Launcher} from './launcher.js';

const DBUS = `<node><interface name="app.hestia.Launcher"><method name="Toggle"/></interface></node>`;
const WM = 'org.gnome.desktop.wm.keybindings';
const KEY = '<Alt>space';

const MAX_BYTES = 20000;
const HINT = 'x-kde-passwordManagerHint';

function settings() {
    const s = {on: true, max: 100, days: 7};
    try {
        const [ok, bytes] = GLib.file_get_contents(GLib.build_filenamev([GLib.get_user_config_dir(), 'hestia', 'settings']));
        if (!ok) return s;
        for (const line of new TextDecoder().decode(bytes).split('\n')) {
            const [k, v] = line.split('=');
            if (k === 'clipboard') s.on = v !== 'off';
            if (k === 'clipboard_max') s.max = Math.min(1000, Math.max(10, parseInt(v) || 100));
            if (k === 'clipboard_days') s.days = Math.min(365, Math.max(1, parseInt(v) || 7));
        }
    } catch (_e) { /* defaults */ }
    return s;
}

export default class HestiaClipboard extends Extension {
    enable() {
        this._launcher = new Launcher();
        try {
            // Alt+Space belongs to the window menu until we ask for it; give it back when we leave
            this._wm = new Gio.Settings({schema_id: WM});
            this._menu = this._wm.get_strv('activate-window-menu');
            if (this._menu.includes(KEY)) this._wm.set_strv('activate-window-menu', this._menu.filter(k => k !== KEY));
            Main.wm.addKeybinding('launcher-key', this.getSettings(), Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW, () => this._launcher.toggle());
            this._keyed = true;
            // `hestia-tray` and anything else can open it too:  gdbus call --session --dest org.gnome.Shell --object-path /app/hestia/Launcher --method app.hestia.Launcher.Toggle
            this._dbus = Gio.DBusExportedObject.wrapJSObject(DBUS, {Toggle: () => this._launcher.toggle()});
            this._dbus.export(Gio.DBus.session, '/app/hestia/Launcher');
        } catch (e) {
            console.error(`hestia: launcher not started: ${e}`);
        }
        this._clipboard = St.Clipboard.get_default();
        this._file = GLib.build_filenamev([GLib.get_user_data_dir(), 'hestia', 'clipboard.jsonl']);
        GLib.mkdir_with_parents(GLib.path_get_dirname(this._file), 0o700);
        this._selection = global.display.get_selection();
        this._id = this._selection.connect('owner-changed', (_sel, type) => {
            if (type === Meta.SelectionType.SELECTION_CLIPBOARD) this._read();
        });
    }

    disable() {
        if (this._keyed) Main.wm.removeKeybinding('launcher-key');
        this._keyed = false;
        try { this._dbus?.unexport(); } catch (_e) { /* not exported */ }
        this._dbus = null;
        if (this._wm && this._menu?.includes(KEY)) this._wm.set_strv('activate-window-menu', this._menu);
        this._wm = null;
        this._launcher?.destroy();
        this._launcher = null;
        if (this._id) this._selection.disconnect(this._id);
        this._id = null;
        this._selection = null;
        this._clipboard = null;
    }

    _read() {
        const cfg = settings();
        if (!cfg.on) return;
        try {
            const types = this._clipboard.get_mimetypes(St.ClipboardType.CLIPBOARD) ?? [];
            if (types.includes(HINT)) return; // a password manager asked not to be recorded
        } catch (_e) { /* no mimetype list: carry on */ }
        this._clipboard.get_text(St.ClipboardType.CLIPBOARD, (_c, text) => {
            if (!text || !text.trim() || text.length > MAX_BYTES) return;
            this._append(text, cfg);
        });
    }

    _append(text, cfg) {
        let entries = [];
        try {
            const [ok, bytes] = GLib.file_get_contents(this._file);
            if (ok) {
                for (const line of new TextDecoder().decode(bytes).split('\n')) {
                    if (!line) continue;
                    try { entries.push(JSON.parse(line)); } catch (_e) { /* skip a bad line */ }
                }
            }
        } catch (_e) { /* first run */ }
        const now = Math.floor(Date.now() / 1000);
        const cutoff = now - cfg.days * 86400;
        entries = entries.filter(e => e.t >= cutoff && e.text !== text); // newest copy wins
        entries.push({t: now, text});
        entries = entries.slice(-cfg.max);
        const body = entries.map(e => JSON.stringify(e)).join('\n') + '\n';
        GLib.file_set_contents(this._file, new TextEncoder().encode(body));
        GLib.chmod(this._file, 0o600);
    }
}
