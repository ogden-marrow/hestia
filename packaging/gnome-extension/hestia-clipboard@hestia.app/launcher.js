// The Hestia launcher, drawn by the shell: a Spotlight-style bar that opens on Alt+Space.
// The searching is done by `hestia --serve`, kept running in the background; this file only draws
// the bar, sends what you type and shows what comes back. No plugins.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const WIDTH = 680;
const ROW = 44;

function palette() {
    let light = false;
    try {
        light = new Gio.Settings({schema_id: 'org.gnome.desktop.interface'}).get_string('color-scheme') === 'prefer-light';
    } catch (_e) { /* dark */ }
    return light
        ? {card: 'rgba(250,250,252,0.97)', line: 'rgba(0,0,0,0.10)', text: '#1d1d1f', dim: 'rgba(0,0,0,0.5)'}
        : {card: 'rgba(32,32,35,0.97)', line: 'rgba(255,255,255,0.10)', text: '#f2f2f4', dim: 'rgba(255,255,255,0.55)'};
}

export class Launcher {
    constructor() {
        this._actor = null;
        this._proc = null;
        this._hits = [];
        this._sel = 0;
        this._grab = null;
        this._closing = false;
        this._rows = [];
    }

    // ---- the engine
    _ensureEngine() {
        if (this._proc) return;
        // installed to ~/.local/bin by install.sh; ~/.cargo/bin when built with cargo install; else whatever PATH finds
        const exe = ['.local/bin', '.cargo/bin'].map(d => GLib.build_filenamev([GLib.get_home_dir(), d, 'hestia']))
            .find(p => GLib.file_test(p, GLib.FileTest.IS_EXECUTABLE)) ?? 'hestia';
        try {
            this._proc = Gio.Subprocess.new([exe, '--serve'], Gio.SubprocessFlags.STDIN_PIPE | Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
        } catch (e) {
            console.error(`hestia launcher: cannot start the engine: ${e}`);
            this._proc = null;
            return;
        }
        const proc = this._proc;
        this._in = proc.get_stdin_pipe();
        this._out = new Gio.DataInputStream({base_stream: proc.get_stdout_pipe()});
        const next = () => {
            this._out.read_line_async(GLib.PRIORITY_DEFAULT, null, (stream, res) => {
                let line = null;
                try { [line] = stream.read_line_finish_utf8(res); } catch (_e) { line = null; }
                if (line === null) {
                    if (this._proc === proc) this._proc = null; // it ended: start again next time
                    return;
                }
                this._reply(line);
                next();
            });
        };
        next();
    }

    _send(obj) {
        this._ensureEngine();
        if (!this._proc) return;
        try {
            this._in.write_all(new TextEncoder().encode(JSON.stringify(obj) + '\n'), null);
            this._in.flush(null);
        } catch (e) {
            console.error(`hestia launcher: engine write failed: ${e}`);
            this._proc = null;
        }
    }

    _reply(line) {
        let v;
        try { v = JSON.parse(line); } catch (_e) { return; }
        if (v.copy !== undefined) {
            St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, v.copy);
        } else if (v.hits && this._actor) {
            this._hits = v.hits;
            this._sel = 0;
            this._render();
        }
    }

    // ---- the bar
    toggle() {
        if (this._actor && !this._closing) this.close(); else this.open();
    }

    open() {
        if (this._actor && this._closing) this._destroy(); // reopened while it was leaving
        if (this._actor) return;
        const c = palette();
        const monitor = Main.layoutManager.currentMonitor ?? Main.layoutManager.primaryMonitor;
        this._actor = new St.Widget({
            reactive: true, x: monitor.x, y: monitor.y, width: monitor.width, height: monitor.height,
            opacity: 0,
        });
        this._actor.connect('button-press-event', () => { this.close(); return Clutter.EVENT_STOP; });

        this._card = new St.BoxLayout({
            orientation: Clutter.Orientation.VERTICAL, reactive: true,
            width: WIDTH, clip_to_allocation: true,
            // placed outright (not by margins): centred across, 14% of the screen height down from the top
            x: Math.round((monitor.width - WIDTH) / 2), y: Math.round(monitor.height * 0.14),
            style: `background-color: ${c.card}; border-radius: 24px; border: 1px solid ${c.line}; box-shadow: 0 18px 50px rgba(0,0,0,0.45); padding: 0;`,
        });
        this._card.connect('button-press-event', () => Clutter.EVENT_STOP);

        const bar = new St.BoxLayout({style: 'padding: 0 22px; height: 64px;', y_align: Clutter.ActorAlign.CENTER});
        const glass = new St.Icon({icon_name: 'system-search-symbolic', icon_size: 24, style: `color: ${c.dim};`, y_align: Clutter.ActorAlign.CENTER});
        this._entry = new St.Entry({
            hint_text: 'Hestia Search', x_expand: true, can_focus: true, y_align: Clutter.ActorAlign.CENTER,
            style: `background: transparent; border: none; box-shadow: none; color: ${c.text}; font-size: 22px; caret-color: ${c.text}; margin-left: 14px; padding: 0;`,
        });
        bar.add_child(glass);
        bar.add_child(this._entry);
        this._card.add_child(bar);

        this._list = new St.BoxLayout({orientation: Clutter.Orientation.VERTICAL, style: 'padding: 4px 8px 10px 8px;', visible: false});
        this._rule = new St.Widget({style: `height: 1px; background-color: ${c.line};`, visible: false});
        this._card.add_child(this._rule);
        this._card.add_child(this._list);
        this._actor.add_child(this._card);
        this._c = c;

        const text = this._entry.clutter_text;
        text.connect('text-changed', () => this._typed());
        text.connect('key-press-event', (_a, ev) => this._key(ev));

        Main.uiGroup.add_child(this._actor);
        this._grab = Main.pushModal(this._actor, {actionMode: Shell.ActionMode.POPUP});
        if (!this._grab) { this._destroy(); return; }
        this._entry.grab_key_focus();
        // it lifts into place: fades in, grows from 94%, settles down from 10px above
        this._actor.opacity = 255;
        this._card.set_pivot_point(0.5, 0.0);
        this._card.set({opacity: 0, scale_x: 0.94, scale_y: 0.94, translation_y: -10});
        this._card.ease({
            opacity: 255, scale_x: 1, scale_y: 1, translation_y: 0,
            duration: 200, mode: Clutter.AnimationMode.EASE_OUT_EXPO,
        });
        this._ensureEngine();
    }

    close() {
        if (!this._actor || this._closing) return;
        if (this._grab) { Main.popModal(this._grab); this._grab = null; }
        // focus goes back at once; the card quietly shrinks away
        this._closing = true;
        const actor = this._actor;
        this._card.ease({
            opacity: 0, scale_x: 0.97, scale_y: 0.97, translation_y: -4,
            duration: 110, mode: Clutter.AnimationMode.EASE_IN_QUAD,
            onComplete: () => { if (this._actor === actor) this._destroy(); },
        });
    }

    _destroy() {
        this._actor?.destroy();
        this._actor = null;
        this._closing = false;
        this._hits = [];
        this._rows = [];
    }

    destroy() {
        this.close();
        try { this._proc?.force_exit(); } catch (_e) { /* already gone */ }
        this._proc = null;
    }

    _typed() {
        const q = this._entry.get_text();
        if (!q.trim()) {
            this._hits = [];
            this._render();
            return;
        }
        this._send({q});
    }

    _key(ev) {
        const k = ev.get_key_symbol();
        if (k === Clutter.KEY_Escape) { this.close(); return Clutter.EVENT_STOP; }
        if (k === Clutter.KEY_Down || (k === Clutter.KEY_Tab)) { this._move(1); return Clutter.EVENT_STOP; }
        if (k === Clutter.KEY_Up || k === Clutter.KEY_ISO_Left_Tab) { this._move(-1); return Clutter.EVENT_STOP; }
        if (k === Clutter.KEY_Return || k === Clutter.KEY_KP_Enter) { this._run(this._sel); return Clutter.EVENT_STOP; }
        return Clutter.EVENT_PROPAGATE;
    }

    _move(d) {
        if (!this._hits.length) return;
        this._sel = (this._sel + d + this._hits.length) % this._hits.length;
        this._paint();
    }

    _select(i) {
        if (this._sel === i) return;
        this._sel = i;
        this._paint();
    }

    _paint() {
        const c = this._c;
        this._rows.forEach((r, i) => {
            const on = i === this._sel;
            r.row.style = `height: ${ROW}px; padding: 0 12px; border-radius: 10px; spacing: 14px;` + (on ? ' background-color: -st-accent-color;' : '');
            r.title.style = `font-size: 15px; color: ${on ? '-st-accent-fg-color' : c.text};`;
            r.sub.style = `font-size: 12px; color: ${on ? '-st-accent-fg-color' : c.dim};`;
        });
    }

    _run(i) {
        if (!this._hits[i]) return;
        this._send({run: i});
        this.close();
    }

    _icon(h) {
        const size = 28;
        if (h.kind === 'app') {
            try {
                const info = Gio.DesktopAppInfo.new_from_filename(h.path);
                const gicon = info?.get_icon();
                if (gicon) return new St.Icon({gicon, icon_size: size});
            } catch (_e) { /* fall through */ }
        }
        const name = {clip: 'edit-paste-symbolic', file: 'text-x-generic-symbolic', screen: 'user-home-symbolic'}[h.kind] ?? 'system-run-symbolic';
        return new St.Icon({icon_name: name, icon_size: size, style: `color: ${this._c.dim};`});
    }

    _render() {
        if (!this._actor) return;
        const c = this._c;
        const before = this._card.height;
        this._list.destroy_all_children();
        this._rows = [];
        const any = this._hits.length > 0;
        this._list.visible = any;
        this._rule.visible = any;
        let last = null;
        let n = 0;
        this._hits.forEach((h, i) => {
            const heading = h.top ? 'Top Hit' : h.group;
            if (heading !== last) {
                const label = new St.Label({text: heading, style: `color: ${c.dim}; font-size: 12px; font-weight: bold; padding: 10px 12px 4px 12px;`});
                this._list.add_child(label);
                this._appear(label, n++);
                last = heading;
            }
            const row = new St.BoxLayout({reactive: true, track_hover: true, y_align: Clutter.ActorAlign.CENTER});
            const title = new St.Label({text: h.title, x_expand: true, y_align: Clutter.ActorAlign.CENTER});
            const sub = new St.Label({text: h.sub ?? '', y_align: Clutter.ActorAlign.CENTER});
            row.add_child(this._icon(h));
            row.add_child(title);
            row.add_child(sub);
            row.connect('button-press-event', () => { this._run(i); return Clutter.EVENT_STOP; });
            row.connect('enter-event', () => { this._select(i); return Clutter.EVENT_PROPAGATE; });
            this._list.add_child(row);
            this._rows.push({row, title, sub});
            this._appear(row, n++);
        });
        this._paint();
        this._grow(before);
    }

    // rows slide up and fade in one after another, like Spotlight's results
    _appear(actor, n) {
        if (n > 9) return;
        actor.set({opacity: 0, translation_y: 6});
        actor.ease({
            opacity: 255, translation_y: 0, delay: Math.min(n, 8) * 16, duration: 170,
            mode: Clutter.AnimationMode.EASE_OUT_CUBIC,
        });
    }

    // the card eases to its new height instead of jumping
    _grow(before) {
        const card = this._card;
        card.remove_all_transitions();
        card.set_height(-1);
        const [, natural] = card.get_preferred_height(WIDTH);
        if (!before || Math.abs(natural - before) < 2) return;
        card.set_height(before);
        card.ease({
            height: natural, duration: 180, mode: Clutter.AnimationMode.EASE_OUT_CUBIC,
            onComplete: () => card.set_height(-1),
        });
    }
}
