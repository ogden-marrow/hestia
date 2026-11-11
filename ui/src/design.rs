//! One design language, informed by the classic Mac OS: crisp 1-pixel rules instead of shadows,
//! flat panes that meet edge to edge, solid highlights, small corner radii, and nothing floating.
//! The user chooses light, dark or auto, an accent colour, spacing and text size.
//!
//! Spacing is one scale of multiples of 4: gap 8, item padding 12, panel padding 16.

use crate::settings::{Accent, Scheme, Settings};
use rune::tok::{self, Family, TextStyle, Theme};
use rune::{oklch, oklcha, Color, El};

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
            Density::Compact => 26.0,
            Density::Cozy => 32.0,
            Density::Airy => 40.0,
        }
    }
    /// Padding inside a panel that holds free-form content.
    pub fn pad(&self) -> f32 {
        match self.density {
            Density::Compact => 12.0,
            Density::Cozy => 16.0,
            Density::Airy => 20.0,
        }
    }
    /// The gap between neighbours.
    pub fn gap(&self) -> f32 {
        self.pad() * 0.5
    }
    /// Horizontal padding inside a row, so text lines up down a whole list.
    pub fn item_px(&self) -> f32 {
        self.pad() * 0.75
    }
    /// Corner radius: classic Mac controls were barely rounded.
    pub fn r(&self, base: f32) -> f32 {
        base * 0.5
    }
    pub fn size(&self, base: f32) -> f32 {
        base * self.type_scale
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
