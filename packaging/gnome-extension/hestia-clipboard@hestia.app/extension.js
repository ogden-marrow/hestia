// Hestia clipboard: when the clipboard changes, append the text to ~/.local/share/hestia/clipboard.jsonl.
// Settings come from ~/.config/hestia/settings (clipboard=on|off, clipboard_max, clipboard_days).
// OFF unless clipboard=on. Text only. Skipped: empty, huge, anything a password manager marks with its password hint,
// and text that looks like a secret or a one-time code. Turning it off deletes the history; old entries are pruned on a timer.
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
    const s = {on: false, max: 100, days: 7};
    try {
        const [ok, bytes] = GLib.file_get_contents(GLib.build_filenamev([GLib.get_user_config_dir(), 'hestia', 'settings']));
        if (!ok) return s;
        for (const line of new TextDecoder().decode(bytes).split('\n')) {
            const [k, v] = line.split('=');
            if (k === 'clipboard') s.on = v === 'on';
            if (k === 'clipboard_max') s.max = Math.min(1000, Math.max(10, parseInt(v) || 100));
            if (k === 'clipboard_days') s.days = Math.min(365, Math.max(1, parseInt(v) || 7));
        }
    } catch (_e) { /* defaults */ }
    return s;
}

// Text that should never be remembered: keys, tokens, one-time codes, long unbroken random strings.
function looksSecret(text) {
    const t = text.trim();
    if (/^\d{4,8}$/.test(t)) return true;                                   // one-time code or PIN
    if (/BEGIN [A-Z ]*PRIVATE KEY/.test(t)) return true;
    if (/(ghp_|gho_|ghs_|github_pat_|sk-|sk_live_|pk_live_|AKIA[0-9A-Z]{8}|xox[baprs]-|AIza|glpat-|npm_)[A-Za-z0-9_\-]{8,}/i.test(t)) return true;
    if (/^eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]*$/.test(t)) return true; // JWT
    if (/(passw(or)?d|secret|token|api[_-]?key)\s*[:=]\s*\S{6,}/i.test(t)) return true;
    if (!/\s/.test(t) && t.length >= 32 && /^[A-Za-z0-9+/=_\-]+$/.test(t)) return true;  // long random-looking string
    return false;
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
        this._prune();
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, 600, () => { this._prune(); return GLib.SOURCE_CONTINUE; });
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
        if (this._timer) GLib.source_remove(this._timer);
        this._timer = null;
    }

    // Off means gone: no history file at all. On means expired entries are dropped even if nothing new is copied.
    _prune() {
        const cfg = settings();
        if (!cfg.on) { this._wipe(); return; }
        const entries = this._load();
        const cutoff = Math.floor(Date.now() / 1000) - cfg.days * 86400;
        const kept = entries.filter(e => e.t >= cutoff).slice(-cfg.max);
        if (kept.length !== entries.length) this._save(kept);
    }

    _wipe() {
        try { Gio.File.new_for_path(this._file).delete(null); } catch (_e) { /* nothing to delete */ }
    }

    _load() {
        const entries = [];
        try {
            const [ok, bytes] = GLib.file_get_contents(this._file);
            if (ok) {
                for (const line of new TextDecoder().decode(bytes).split('\n')) {
                    if (!line) continue;
                    try { entries.push(JSON.parse(line)); } catch (_e) { /* skip a bad line */ }
                }
            }
        } catch (_e) { /* first run */ }
        return entries;
    }

    // created 0600 from the first byte: never world-readable, even briefly
    _save(entries) {
        const body = entries.map(e => JSON.stringify(e)).join('\n') + (entries.length ? '\n' : '');
        GLib.file_set_contents_full(this._file, new TextEncoder().encode(body), GLib.FileSetContentsFlags.CONSISTENT, 0o600);
    }

    _read() {
        const cfg = settings();
        if (!cfg.on) { this._wipe(); return; }
        try {
            const types = this._clipboard.get_mimetypes(St.ClipboardType.CLIPBOARD) ?? [];
            if (types.includes(HINT)) return; // a password manager asked not to be recorded
        } catch (_e) { /* no mimetype list: carry on */ }
        this._clipboard.get_text(St.ClipboardType.CLIPBOARD, (_c, text) => {
            if (!text || !text.trim() || text.length > MAX_BYTES || looksSecret(text)) return;
            this._append(text, cfg);
        });
    }

    _append(text, cfg) {
        let entries = this._load();
        const now = Math.floor(Date.now() / 1000);
        const cutoff = now - cfg.days * 86400;
        entries = entries.filter(e => e.t >= cutoff && e.text !== text); // newest copy wins
        entries.push({t: now, text});
        this._save(entries.slice(-cfg.max));
    }
}
