//! The question queue. One folder at a time, with every reason Hestia has for a label:
//! what the two models think, how you filed neighbouring folders, and how the same kinds of
//! files are filed elsewhere. One click files the folder (and, if you like, its similar neighbours).

use super::{kv, style, Pane, State};
use crate::design::{Design, Surface};
use lifestream::bundle::{Bundle, KINDS};
use lifestream::human;
use rune::tok::{self, type_};
use rune::{col, row, text, Color, El, ScrollSpec, TextStyle};

pub struct Choice {
    pub kind: String,
    pub reasons: Vec<String>,
}

/// The project a folder belongs to: its first three path parts (`~/Downloads/Tachyons a0840`).
pub fn project_of(root: &str) -> String {
    root.split('/').take(3).collect::<Vec<_>>().join("/")
}

/// Other queued folders in the same project, which an answer can cover too.
pub fn siblings(bundles: &[Bundle], i: usize) -> Vec<usize> {
    let Some(b) = bundles.get(i) else { return vec![] };
    let project = project_of(&b.root);
    if project.matches('/').count() < 2 {
        return vec![]; // a top-level folder has no "project" around it
    }
    bundles.iter().enumerate().filter(|(j, o)| *j != i && project_of(&o.root) == project).map(|(j, _)| j).collect()
}

fn add(out: &mut Vec<Choice>, kind: &str, reason: String) {
    match out.iter_mut().find(|c| c.kind == kind) {
        Some(c) => c.reasons.push(reason),
        None => out.push(Choice { kind: kind.to_string(), reasons: vec![reason] }),
    }
}

/// Every label worth offering, best-supported first.
pub fn choices(s: &State, b: &Bundle) -> Vec<Choice> {
    let mut out: Vec<Choice> = Vec::new();
    if let Some(c) = s.suggestions.get(&b.root) {
        for (n, g) in c.suggestions.iter().enumerate() {
            if g.kind != "unsure" {
                add(&mut out, &g.kind, format!("Model {}: {}", if n == 0 { "A" } else { "B" }, g.what));
            }
        }
    }
    let project = project_of(&b.root);
    let mut nearby: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in s.recs.iter().filter(|r| r.by.tier == "human" && r.status != "gone") {
        if r.sources.first().is_some_and(|p| p.starts_with(&project)) {
            *nearby.entry(r.kind.as_str()).or_default() += 1;
        }
    }
    let mut nearby: Vec<_> = nearby.into_iter().collect();
    nearby.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (kind, n) in nearby.into_iter().take(2) {
        add(&mut out, kind, format!("You filed {n} files in this project as this"));
    }
    for (ext, _) in b.exts.iter().take(3) {
        if let Some((kind, n)) = s.ext_kinds.get(ext) {
            add(&mut out, kind, format!("Other {ext} files are filed as this ({n} of them)"));
        }
    }
    out.sort_by(|a, b| b.reasons.len().cmp(&a.reasons.len()).then_with(|| a.kind.cmp(&b.kind)));
    out
}

/// Three file names and "and 3 more", not a run-on list.
pub fn sample_line(samples: &[String], total: usize) -> String {
    let shown: Vec<&str> = samples.iter().take(3).map(String::as_str).collect();
    let more = total.saturating_sub(shown.len());
    match (shown.is_empty(), more) {
        (true, _) => String::new(),
        (false, 0) => shown.join(", "),
        (false, n) => format!("{} and {n} more", shown.join(", ")),
    }
}

pub fn chip(d: &Design, label: String, strong: bool) -> El {
    let t = d.theme();
    let acc = d.accent_color();
    row().h(d.size(22.0)).px(d.gap()).center_y().radius(99.0)
        .fill(if strong { acc.alpha(0.18) } else { t.control_track })
        .child(text(label).font(style(d, type_::LABEL)).color(if strong { acc } else { t.text_secondary }))
}

pub fn button<F: Fn(&mut State) + 'static>(d: &Design, key: &str, label: String, primary: bool, press: F) -> El {
    let t = d.theme();
    row().key(key.to_string()).h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM))
        .fill(if primary { d.accent_color() } else { rune::Color::TRANSPARENT })
        .border(1.0, if primary { d.accent_color() } else { t.card_border })
        .on_press(press)
        .child(text(label).font(style(d, type_::LABEL)).color(if primary { d.on_accent() } else { t.text }))
}

/// The screen's one primary action: accent fill, the tall hit target.
pub fn primary<F: Fn(&mut State) + 'static>(d: &Design, key: &str, label: &str, press: F) -> El {
    row().key(key.to_string()).h(crate::design::hit::PRIMARY).px(d.pad() + crate::design::space::XS).center_y().radius(d.r(tok::radius::SM))
        .fill(d.accent_color()).on_press(press)
        .child(text(label.to_string()).font(d.text(crate::design::Role::Body)).color(d.on_accent()))
}

/// A secondary action that stays out of the way: text only, no rule.
pub fn quiet<F: Fn(&mut State) + 'static>(d: &Design, key: &str, label: &str, press: F) -> El {
    row().key(key.to_string()).h(crate::design::hit::PRIMARY).px(d.pad()).center_y().radius(d.r(tok::radius::SM))
        .on_press(press)
        .child(text(label.to_string()).font(d.text(crate::design::Role::Body)).color(d.theme().text_secondary))
}

/// A button for something that takes time: while it runs it is dimmed, cannot be pressed, and says what it is doing.
pub fn action<F: Fn(&mut State) + 'static>(d: &Design, key: &str, label: &str, busy_label: &str, primary: bool, busy: bool, press: F) -> El {
    let t = d.theme();
    let b = row().key(key.to_string()).h(d.row_h()).px(d.pad()).center_y().radius(d.r(tok::radius::SM))
        .fill(if busy { t.control_track } else if primary { d.accent_color() } else { rune::Color::TRANSPARENT })
        .border(1.0, if busy { t.card_border } else if primary { d.accent_color() } else { t.card_border })
        .child(text(if busy { busy_label } else { label }.to_string()).font(style(d, type_::LABEL)).color(if busy { t.text_secondary } else if primary { d.on_accent() } else { t.text }));
    if busy { b } else { b.on_press(press) }
}

pub fn heading(d: &Design, label: &str) -> El {
    text(label.to_string()).font(style(d, type_::SECTION)).color(d.theme().text_secondary)
}

pub fn panels(s: &State) -> Vec<El> {
    let d = s.design;
    let t = d.theme();
    let acc = d.accent_color();
    let scroll = || ScrollSpec { stick_bottom: false };

    // ---- the queue
    let mut list = col().w_full().gap(d.gap() * 0.5);
    for (i, b) in s.bundles.iter().enumerate() {
        let on = i == s.qsel;
        let sug = s.suggestions.get(&b.root);
        let agree = sug.is_some_and(|c| c.agree());
        let skipped = s.skipped.contains(&b.root);
        let (parent, name) = b.root.trim_start_matches("~/").rsplit_once('/').map(|(a, n)| (a.to_string(), n.to_string())).unwrap_or((String::new(), b.root.clone()));
        let mut chips = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap() * 0.5).flex_wrap();
        match sug {
            Some(c) if c.agree() => chips = chips.child(chip(&d, format!("Both models: {}", crate::kinds::label(&c.suggestions[0].kind)), true)),
            Some(c) if !c.suggestions.is_empty() => {
                for g in &c.suggestions {
                    chips = chips.child(chip(&d, crate::kinds::label(&g.kind), false));
                }
            }
            _ => {}
        }
        if skipped {
            chips = chips.child(chip(&d, "skipped".into(), false));
        }
        list = list.child(
            col().key(format!("bundle-{i}")).w_full().px(d.item_px()).py(d.gap()).gap(d.gap() * 0.5).radius(d.r(tok::radius::SM))
                .fill(if on { d.select() } else { rune::Color::TRANSPARENT })
                .border(1.0, if on { acc } else { Color::TRANSPARENT })
                .on_press(move |s: &mut State| { s.qsel = i; s.msg.clear(); s.kinds_open = false })
                .child(row().w_full().gap(d.gap()).center_y()
                    .child(text(name).font(style(&d, type_::TITLE)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                    .child(text(format!("{} files", b.ids.len())).font(style(&d, type_::LABEL)).color(if agree { acc } else { t.text_secondary })))
                .child(text(parent).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).truncate())
                .child(chips),
        );
    }
    let center = col().grow(1.0).shrink().min_w(0.0).padding(d.gap()).gap(d.gap()).panel(&d)
        .child(col().w_full().px(d.item_px()).py(d.gap() * 0.5).gap(2.0)
            .child(text("Questions").font(style(&d, type_::TITLE)).color(t.text))
            .child(text({
                let lead = if s.bundles.is_empty() { "Hestia has no questions for you right now.".to_string() } else { format!("{} need a quick answer. Your answer will also be used for similar folders.", crate::today::count(s.bundles.len(), "folder", "folders")) };
                if s.msg.is_empty() { lead } else { format!("{lead}  ·  {}", s.msg) }
            }).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()))
        .child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(list));

    // ---- the answer
    let mut detail = col().w(380.0).padding(d.pad()).gap(d.pad()).panel(&d);
    if let Some(b) = s.bundles.get(s.qsel) {
        let sibs = siblings(&s.bundles, s.qsel);
        let mut body = col().w_full().shrink().min_w(0.0).gap(d.pad())
            .child(col().w_full().gap(4.0)
                .child(text(b.root.rsplit('/').next().unwrap_or(&b.root).to_string()).font(TextStyle { size: d.size(17.0), ..style(&d, type_::TITLE) }).color(t.text).wrap_anywhere().shrink())
                .child(text(b.root.clone()).font(style(&d, type_::MONO_CAPTION)).color(t.text_secondary).wrap_anywhere().shrink())
                .child(text(format!("{} files · {}", b.ids.len(), human(b.size))).font(style(&d, type_::LABEL)).color(t.text_secondary)));

        let mut exts = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap() * 0.5).flex_wrap();
        for (e, n) in &b.exts {
            exts = exts.child(chip(&d, format!("{e} ×{n}"), false));
        }
        body = body.child(col().w_full().gap(d.gap()).child(heading(&d, "What's in it")).child(exts)
            .child(text(sample_line(&b.samples, b.ids.len())).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink()));

        // suggestions, best first
        let mut sugg = col().w_full().gap(d.gap()).child(heading(&d, "Suggestions"));
        let all = choices(s, b);
        if all.is_empty() {
            sugg = sugg.child(text("Nothing to go on yet. Pick a kind below.").font(style(&d, type_::LABEL)).color(t.text_secondary));
        }
        for (n, c) in all.iter().enumerate() {
            let kind = c.kind.clone();
            let mut card = col().w_full().padding(d.item_px()).gap(d.gap() * 0.5).tile(&d)
                .child(row().w_full().gap(d.gap()).center_y()
                    .child(text(crate::kinds::label(&c.kind)).font(style(&d, type_::TITLE)).color(t.text).grow(1.0).shrink().min_w(0.0).truncate())
                    .child(if n == 0 && c.reasons.len() > 1 { chip(&d, "best guess".into(), true) } else { col().size(0.0, 0.0) }));
            for r in &c.reasons {
                card = card.child(text(format!("•  {r}")).font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink());
            }
            card = card.child(row().child(button(&d, &format!("accept-{}-{n}", b.root), format!("File {} as this", if sibs.is_empty() || !s.settings.siblings { "folder".to_string() } else { format!("{} folders", sibs.len() + 1) }), n == 0, move |s: &mut State| s.answer(&kind, &[]))));
            sugg = sugg.child(card);
        }
        body = body.child(sugg);

        if !sibs.is_empty() {
            let names: Vec<String> = sibs.iter().take(3).map(|&j| s.bundles[j].root.rsplit('/').next().unwrap_or("").to_string()).collect();
            body = body.child(
                text(format!("{} similar {} nearby ({}) {}. Change this in Settings.", sibs.len(), if sibs.len() == 1 { "folder" } else { "folders" }, names.join(", "),
                    if s.settings.siblings { "will be answered together with this one" } else { "are not included" }))
                    .font(style(&d, type_::LABEL)).color(t.text_secondary).wrap_anywhere().shrink(),
            );
        }

        // something else: tucked away until asked for; every kind is then one click away
        let mut other = col().w_full().gap(d.gap()).child(heading(&d, "Something else"));
        if s.kinds_open {
            let mut kinds = row().w_full().shrink().min_w(0.0).gaps(d.gap(), d.gap()).flex_wrap();
            for (n, k) in KINDS.iter().enumerate() {
                let k = k.to_string();
                kinds = kinds.child(
                    row().key(format!("kind-{}-{n}", b.root)).h(d.size(26.0)).px(d.gap()).center_y().radius(99.0).fill(t.control_off)
                        .on_press({ let k = k.clone(); move |s: &mut State| s.answer(&k, &[]) })
                        .child(text(k).font(style(&d, type_::LABEL)).color(t.text)),
                );
            }
            other = other.child(kinds);
        } else {
            other = other.child(row().child(button(&d, "kinds-more", "Choose another kind…".into(), false, |s: &mut State| s.kinds_open = true)));
        }
        body = body.child(other
            .child(super::field(&d, "teach", &s.teach, "or type your own kind ↵", |s: &mut State| &mut s.teach).on_submit(|s: &mut State, _: &mut rune::Tx| {
                let text = s.teach.text.trim().to_string();
                let mut p = text.split_whitespace();
                if let Some(kind) = p.next() {
                    let tags: Vec<String> = p.map(String::from).collect();
                    s.answer(kind, &tags);
                }
            })));

        let root = b.root.clone();
        body = body.child(row().w_full().gap(d.gap()).child(button(&d, &format!("skip-{}", b.root), "Skip for now".into(), false, move |s: &mut State| {
            s.skipped.push(root.clone());
        })));
        detail = detail.child(col().w_full().scroll(scroll()).shrink().min_h(0.0).grow(1.0).child(body));
    } else {
        detail = detail.child(text("Nothing left to answer.").font(style(&d, type_::BODY)).color(t.text_secondary));
    }
    let _ = (kv, Pane::Questions);
    vec![center, detail]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(root: &str) -> Bundle {
        Bundle { root: root.into(), ids: vec!["x".into()], size: 1, exts: vec![], samples: vec![] }
    }

    #[test]
    fn folders_in_the_same_project_are_siblings() {
        let bs = vec![b("~/Downloads/Tachyons a0840/server"), b("~/Downloads/Tachyons a0840/client"), b("~/Downloads/Other/x"), b("~/Saab")];
        assert_eq!(project_of(&bs[0].root), "~/Downloads/Tachyons a0840");
        assert_eq!(siblings(&bs, 0), vec![1]);
        assert!(siblings(&bs, 2).is_empty(), "a project of one has no siblings");
        assert!(siblings(&bs, 3).is_empty(), "a top-level folder has no project around it");
    }
}

#[cfg(test)]
mod sample_tests {
    use super::sample_line;

    #[test]
    fn a_short_list_of_names_not_a_run_on() {
        let s: Vec<String> = ["a.pdf", "b.pdf", "c.pdf", "d.pdf"].iter().map(|x| x.to_string()).collect();
        assert_eq!(sample_line(&s, 6), "a.pdf, b.pdf, c.pdf and 3 more");
        assert_eq!(sample_line(&s[..2], 2), "a.pdf, b.pdf");
        assert_eq!(sample_line(&[], 0), "");
    }
}
