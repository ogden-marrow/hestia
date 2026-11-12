//! Settings → Domains: the big areas of your computer, editable. Each domain says what belongs in it
//! (told to the local classifier in plain words), which file kinds it owns, where it lives and how it is
//! backed up. Edits are saved as `domains/<id>.toml` in your home repository, over the stock domain of
//! the same id; "Reset" deletes that file. A new id makes a new domain.

use super::{field, style, State};
use crate::design::Design;
use crate::prefs::{segmented, setting};
use crate::questions::button;
use lifestream::domains::{self, Domain};
use rune::tok::{self, type_};
use rune::{col, row, text, El, TextEdit, Tx};

const SHAPES: [&str; 5] = ["by_topic", "by_year", "by_source", "by_project", "flat"];
const GIT_MB: [u32; 4] = [0, 1, 5, 10];
const STORES: [&str; 2] = ["encrypted", "none"];

fn csv(v: &[String]) -> String {
    v.join(", ")
}

impl State {
    fn domains_dir(&self) -> std::path::PathBuf {
        self.repo_dir.join("domains")
    }
    pub(crate) fn reload_domains(&mut self) {
        self.domains = domains::load(Some(&self.domains_dir())).unwrap_or_else(|_| domains::defaults());
        self.dom_sel = self.dom_sel.min(self.domains.len().saturating_sub(1));
    }
    pub(crate) fn select_domain(&mut self, i: usize) {
        self.dom_sel = i;
        if let Some(d) = self.domains.get(i).cloned() {
            self.d_name = TextEdit::new(&d.name);
            self.d_desc = TextEdit::new(&d.description);
            self.d_kinds = TextEdit::new(csv(&d.kinds));
            self.d_folder = TextEdit::new(&d.folder);
            self.dom_edit = d;
        }
        self.msg.clear();
    }
    /// Write the edited domain over the stock one and make sure the whole set still loads.
    fn save_domain(&mut self) {
        let mut d = self.dom_edit.clone();
        d.name = self.d_name.text.trim().to_string();
        d.description = self.d_desc.text.trim().to_string();
        d.kinds = self.d_kinds.text.split([',', ' ']).map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect();
        d.folder = self.d_folder.text.trim().trim_matches('/').to_string();
        if !d.folder.is_empty() && !lifestream::tree::rel_ok(&d.folder) {
            self.msg = "the folder must be a visible folder inside your home folder: no hidden folders, no .. and no empty parts".into();
            return;
        }
        if d.name.is_empty() || d.description.is_empty() {
            self.msg = "a domain needs a name and a description: the description is what the classifier reads".into();
            return;
        }
        let dir = self.domains_dir();
        let file = dir.join(format!("{}.toml", d.id));
        let before = std::fs::read_to_string(&file).ok();
        let write = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, toml::to_string_pretty(&d).unwrap_or_default()));
        if let Err(e) = write {
            self.msg = format!("not saved: {e}");
            return;
        }
        match domains::load(Some(&dir)) {
            Ok(all) => {
                self.domains = all;
                self.msg = format!("saved domains/{}.toml; the next suggestions use it", d.id);
            }
            Err(e) => {
                // put the file back as it was
                match before {
                    Some(b) => { let _ = std::fs::write(&file, b); }
                    None => { let _ = std::fs::remove_file(&file); }
                }
                self.msg = format!("not saved: {e:#}");
            }
        }
    }
    fn reset_domain(&mut self) {
        let id = self.dom_edit.id.clone();
        let _ = std::fs::remove_file(self.domains_dir().join(format!("{id}.toml")));
        self.reload_domains();
        let i = self.domains.iter().position(|d| d.id == id).unwrap_or(0);
        self.select_domain(i);
        self.msg = "back to the stock definition (a domain you made yourself is removed)".into();
    }
    fn hide_domain(&mut self) {
        let mut d = self.dom_edit.clone();
        d.disabled = true;
        let dir = self.domains_dir();
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(format!("{}.toml", d.id)), toml::to_string_pretty(&d).unwrap_or_default())).is_ok() {
            self.reload_domains();
            self.select_domain(0);
            self.msg = format!("{} is hidden (Reset on the stock list brings it back by deleting domains/{}.toml)", d.name, d.id);
        }
    }
    fn add_domain(&mut self) {
        let name = self.d_new.text.trim().to_string();
        let id: String = lifestream::machine::slug(&name);
        if id.is_empty() {
            return;
        }
        if self.domains.iter().any(|d| d.id == id) {
            self.msg = format!("there is already a domain called {id}");
            return;
        }
        let d = Domain { id: id.clone(), name, description: "Describe what belongs here, in plain words: the classifier reads this.".into(), shape: "by_topic".into(), ..Default::default() };
        let dir = self.domains_dir();
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(format!("{id}.toml")), toml::to_string_pretty(&d).unwrap_or_default())).is_ok() {
            self.d_new.clear();
            self.reload_domains();
            let i = self.domains.iter().position(|x| x.id == id).unwrap_or(0);
            self.select_domain(i);
            self.msg = "new domain: write its description and kinds, then Save".into();
        }
    }
}

fn pick(d: &Design, key: &str, label: &str, on: bool, i: usize) -> El {
    let t = d.theme();
    let acc = d.accent_color();
    row().key(format!("{key}-{i}")).h(d.row_h()).px(d.item_px()).center_y().radius(d.r(tok::radius::SM))
        .fill(if on { acc } else { rune::Color::TRANSPARENT })
        .on_press(move |s: &mut State| s.select_domain(i))
        .child(text(label.to_string()).font(style(d, type_::LABEL)).color(if on { d.on_accent() } else { t.text }))
}

/// A setting whose control is a wide text field: title and words above, the field under them.
fn wide(d: &Design, title: &str, desc: &str, control: El) -> El {
    let t = d.theme();
    col().w_full().gap(d.gap() * 0.6).py(d.gap()).border_sides([0.0, 0.0, 1.0, 0.0], t.card_border)
        .child(text(title.to_string()).font(style(d, type_::BODY)).color(t.text))
        .child(text(desc.to_string()).font(style(d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink())
        .child(control)
}

pub fn tab(s: &State) -> El {
    let d = s.design;
    let t = d.theme();
    let mut list = row().w_full().gaps(d.gap(), d.gap() * 0.5).flex_wrap();
    for (i, dm) in s.domains.iter().enumerate() {
        list = list.child(pick(&d, "dom", &dm.name, i == s.dom_sel, i));
    }
    let e = &s.dom_edit;
    let shape = SHAPES.iter().position(|x| *x == e.shape).unwrap_or(0);
    let git = GIT_MB.iter().position(|x| *x == e.backup.git_max_mb).unwrap_or(2);
    let store = STORES.iter().position(|x| *x == e.backup.store).unwrap_or(0);
    col().w_full().gap(0.0)
        .child(text("The big things you do on a computer. Each one tells Hestia what belongs in it; the local classifier reads these words when it decides where something goes.").font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink().py(d.gap()))
        .child(list)
        .child(wide(&d, "Name", &format!("id: {}", e.id), field(&d, "dom-name", &s.d_name, "name", |s: &mut State| &mut s.d_name)))
        .child(wide(&d, "What belongs here", "Plain words. This is the sentence the models read.", field(&d, "dom-desc", &s.d_desc, "what belongs here", |s: &mut State| &mut s.d_desc)))
        .child(wide(&d, "File kinds it owns", "Like document/pdf, image/raw. A kind belongs to one domain.", field(&d, "dom-kinds", &s.d_kinds, "document/text, document/pdf", |s: &mut State| &mut s.d_kinds)))
        .child(wide(&d, "Folder", "Where it lives under your home folder. Empty for a domain that holds no files.", field(&d, "dom-folder", &s.d_folder, "Documents", |s: &mut State| &mut s.d_folder)))
        .child(setting(&d, "Layout", "How its folder is organised.",
            segmented(&d, "dom-shape", &["Topic", "Year", "Source", "Project", "Flat"], shape, |s, i| s.dom_edit.shape = SHAPES[i].to_string())))
        .child(setting(&d, "Kept in git up to", "Files up to this size are tracked in git; larger ones become pointers into the encrypted store.",
            segmented(&d, "dom-git", &["Pointers only", "1 MB", "5 MB", "10 MB"], git, |s, i| s.dom_edit.backup.git_max_mb = GIT_MB[i])))
        .child(setting(&d, "Large files go to", "Encrypted on this computer before they leave it, with keys only you hold.",
            segmented(&d, "dom-store", &["Encrypted store", "Nowhere"], store, |s, i| s.dom_edit.backup.store = STORES[i].to_string())))
        .child(row().w_full().gap(d.gap()).py(d.pad()).flex_wrap()
            .child(button(&d, "dom-save", "Save".into(), true, |s: &mut State| s.save_domain()))
            .child(button(&d, "dom-reset", "Reset".into(), false, |s: &mut State| s.reset_domain()))
            .child(button(&d, "dom-hide", "Hide".into(), false, |s: &mut State| s.hide_domain())))
        .child(wide(&d, "Add a domain", "Name it, press Return, then describe it.", field(&d, "dom-new", &s.d_new, "Recipes ↵", |s: &mut State| &mut s.d_new).on_submit(|s: &mut State, _: &mut Tx| s.add_domain())))
}
