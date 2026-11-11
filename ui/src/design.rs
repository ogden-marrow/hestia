//! One design language, informed by the classic Mac OS: crisp 1-pixel rules instead of shadows,
//! flat panes that meet edge to edge, solid highlights, small corner radii, and nothing floating.
//! The user chooses light, dark or auto, an accent colour, spacing and text size.
//!
//! This file is the single source for every visual token; `docs/DESIGN.md` explains why each
//! value is what it is. Screens never write a literal size, radius, weight or duration: they ask
//! for a token here. A literal in a screen is a bug.

// Tokens are adopted screen by screen (docs/DESIGN.md, "Adoption"); until a surface moves over,
// some are only exercised by the tests below.
#![allow(dead_code)]

use crate::settings::{Accent, Scheme, Settings};
use rune::tok::{self, Family, TextStyle, Theme};
use rune::{oklch, oklcha, Color, El};

/// Spacing: one 4pt grid. Nothing in the app is spaced by any other number.
pub mod space {
    pub const HAIR: f32 = 2.0; // icon-to-label inside one control, never between controls
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0; // between the major groups on a screen
}

/// Corner radii at 100% density. Concentric rule: an inner radius is the outer radius minus the
/// padding between them, never smaller than `CONTROL`.
pub mod radius {
    pub const CONTROL: f32 = 4.0; // buttons, fields, rows
    pub const CARD: f32 = 6.0; // tiles and cards
    pub const SHEET: f32 = 10.0; // sheets and popovers, the only floating things
    pub const PILL: f32 = 99.0; // chips and badges
}

/// Heights of things you can hit. 44 is the floor for a primary target with a pointer-less finger;
/// a pointer control may be 28 but must be reachable by keyboard.
pub mod hit {
    pub const MIN: f32 = 28.0;
    pub const PRIMARY: f32 = 36.0;
}

/// Motion: durations in milliseconds. Motion explains a change; it never decorates.
pub mod motion {
    pub const INSTANT: u32 = 100; // hover and press feedback
    pub const QUICK: u32 = 180; // selection, toggles, a row appearing
    pub const SETTLE: u32 = 280; // a pane or sheet arriving
    /// With "reduce motion" on, every duration is zero and state changes only crossfade.
    pub const REDUCED: u32 = 0;
}

/// The type scale. Six roles, three weights (400, 500, 600). A screen has exactly one `Heading`
/// (or `Display` on Welcome); everything else is `Body` or smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 28 / 500. The single moment on first launch and the empty states' headline.
    Display,
    /// 20 / 500. The pane's one title.
    Heading,
    /// 15 / 600. A group inside a pane, a card's name.
    Title,
    /// 13 / 400. Reading text and row names.
    Body,
    /// 12 / 500. Secondary information, button labels, column headings.
    Label,
    /// 11 / 500. Counts, paths and timestamps; set in the mono family.
    Caption,
}

impl Role {
    /// (size in points at 100%, weight)
    pub const fn spec(self) -> (f32, f32) {
        match self {
            Role::Display => (28.0, 500.0),
            Role::Heading => (20.0, 500.0),
            Role::Title => (15.0, 600.0),
            Role::Body => (13.0, 400.0),
            Role::Label => (12.0, 500.0),
            Role::Caption => (11.0, 500.0),
        }
    }
    pub const ALL: [Role; 6] = [Role::Display, Role::Heading, Role::Title, Role::Body, Role::Label, Role::Caption];
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Density {
    Compact,
    Cozy,
    Airy,
}

#[derive(Clone, Copy, Debug)]
pub struct Design {
    pub density: Density,
    pub type_scale: f32,
    pub dark: bool,
    pub accent: Color,
}

impl Default for Design {
    fn default() -> Self {
        Design::build(&Settings::default(), (true, oklch(0.7, 0.19, 5.0)))
    }
}

fn pick(a: Accent, system: Color) -> Color {
    match a {
        Accent::System => system,
        Accent::Blue => oklch(0.62, 0.19, 255.0),
        Accent::Teal => oklch(0.66, 0.12, 190.0),
        Accent::Green => oklch(0.64, 0.17, 150.0),
        Accent::Orange => oklch(0.7, 0.17, 55.0),
        Accent::Purple => oklch(0.6, 0.2, 305.0),
        Accent::Pink => oklch(0.66, 0.19, 5.0),
        Accent::Graphite => oklch(0.5, 0.02, 260.0),
    }
}

impl Design {
    /// Light, dark or auto (the desktop's setting), the chosen accent, spacing and text size.
    pub fn build(s: &Settings, desktop: (bool, Color)) -> Design {
        Design {
            density: s.density,
            type_scale: s.text,
            dark: match s.scheme { Scheme::Auto => desktop.0, Scheme::Light => false, Scheme::Dark => true },
            accent: pick(s.accent, desktop.1),
        }
    }

    pub fn theme(&self) -> Theme {
        let mut t = tok::theme(self.dark);
        if self.dark {
            t.bg = oklch(0.15, 0.004, 260.0);
            t.card = oklch(0.215, 0.004, 260.0);
            t.card_border = oklcha(1.0, 0.0, 0.0, 0.16); // the rule between panes
            t.control_off = oklcha(1.0, 0.0, 0.0, 0.10);
            t.control_track = oklcha(1.0, 0.0, 0.0, 0.07);
        } else {
            t.bg = oklch(0.9, 0.003, 260.0);
            t.card = oklch(0.998, 0.0, 0.0);
            t.card_border = oklcha(0.15, 0.01, 265.0, 0.30);
            t.control_off = oklcha(0.2, 0.01, 265.0, 0.10);
            t.control_track = oklcha(0.2, 0.01, 265.0, 0.06);
        }
        t
    }

    /// A quiet wash for a selected row inside a list.
    pub fn select(&self) -> Color {
        self.accent.alpha(if self.dark { 0.24 } else { 0.18 })
    }

    /// The sidebar's own tone, a step off the content.
    pub fn side_fill(&self) -> Color {
        if self.dark { oklch(0.19, 0.004, 260.0) } else { oklch(0.94, 0.002, 260.0) }
    }

    pub fn accent_color(&self) -> Color {
        self.accent
    }

    /// Text on top of a solid accent fill.
    pub fn on_accent(&self) -> Color {
        oklch(0.99, 0.0, 0.0)
    }

    /// Height of a list row or a button.
    pub fn row_h(&self) -> f32 {
        match self.density {
            Density::Compact => hit::MIN,
            Density::Cozy => 32.0,
            Density::Airy => hit::PRIMARY + space::XS,
        }
    }
    /// Padding inside a panel that holds free-form content.
    pub fn pad(&self) -> f32 {
        match self.density {
            Density::Compact => space::MD,
            Density::Cozy => space::LG,
            Density::Airy => space::LG + space::XS,
        }
    }
    /// The gap between neighbours: always on the 4pt grid.
    pub fn gap(&self) -> f32 {
        match self.density {
            Density::Compact => space::XS,
            Density::Cozy => space::SM,
            Density::Airy => space::MD,
        }
    }
    /// Horizontal padding inside a row, so text lines up down a whole list.
    pub fn item_px(&self) -> f32 {
        match self.density {
            Density::Compact => space::SM,
            Density::Cozy => space::MD,
            Density::Airy => space::LG,
        }
    }
    /// The space between major groups on a screen.
    pub fn section(&self) -> f32 {
        space::XXL
    }
    /// Corner radius: classic Mac controls were barely rounded.
    pub fn r(&self, base: f32) -> f32 {
        base * 0.5
    }
    pub fn size(&self, base: f32) -> f32 {
        base * self.type_scale
    }

    /// A text style for a role, in the app's typeface and the user's text size.
    pub fn text(&self, role: Role) -> TextStyle {
        let (size, weight) = role.spec();
        let family = if role == Role::Caption { Family::SystemMono } else { Family::Named("Ubuntu Sans") };
        TextStyle { size: self.size(size), family, ..TextStyle::new(size, weight) }
    }

    /// A text style in the app's typeface and scale.
    pub fn font(&self, base: TextStyle) -> TextStyle {
        TextStyle { size: self.size(base.size), family: Family::Named("Ubuntu Sans"), ..base }
    }
}

/// Surfaces: crisp, flat, meeting edge to edge.
pub trait Surface {
    /// The whole-window backdrop.
    fn backdrop(self, d: &Design) -> El;
    /// A main pane: flat, with a 1-pixel rule on its left edge.
    fn panel(self, d: &Design) -> El;
    /// The sidebar: its own tone.
    fn side(self, d: &Design) -> El;
    /// A tile inside a pane: a ruled box.
    fn tile(self, d: &Design) -> El;
}

impl Surface for El {
    fn backdrop(self, d: &Design) -> El {
        self.fill(d.theme().bg)
    }

    fn panel(self, d: &Design) -> El {
        let t = d.theme();
        self.fill(t.card).border_sides([0.0, 0.0, 0.0, 1.0], t.card_border)
    }

    fn side(self, d: &Design) -> El {
        self.fill(d.side_fill())
    }

    fn tile(self, d: &Design) -> El {
        let t = d.theme();
        self.radius(d.r(tok::radius::MD)).fill(t.control_track).border(1.0, t.card_border)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_is_on_the_4pt_grid() {
        for v in [space::XS, space::SM, space::MD, space::LG, space::XL, space::XXL] {
            assert_eq!(v % 4.0, 0.0, "{v}");
        }
        for density in [Density::Compact, Density::Cozy, Density::Airy] {
            let d = Design { density, ..Design::default() };
            assert_eq!(d.pad() % 4.0, 0.0);
            assert_eq!(d.gap() % 4.0, 0.0, "{density:?} gap is off-grid");
            assert_eq!(d.item_px() % 4.0, 0.0, "{density:?} item padding is off-grid");
            assert!(d.row_h() >= hit::MIN, "{density:?} rows are below the hit-target floor");
        }
    }

    #[test]
    fn type_scale_descends_and_uses_three_weights() {
        let sizes: Vec<f32> = Role::ALL.iter().map(|r| r.spec().0).collect();
        assert!(sizes.windows(2).all(|w| w[0] > w[1]), "{sizes:?}");
        for r in Role::ALL {
            assert!([400.0, 500.0, 600.0].contains(&r.spec().1), "{r:?}");
        }
    }

    #[test]
    fn text_follows_the_users_text_size() {
        let d = Design { type_scale: 1.25, ..Design::default() };
        assert_eq!(d.text(Role::Body).size, 13.0 * 1.25);
    }

    #[test]
    fn radii_nest() {
        assert!(radius::CONTROL < radius::CARD && radius::CARD < radius::SHEET);
    }
}
