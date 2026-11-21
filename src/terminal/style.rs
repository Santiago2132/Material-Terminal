//! ANSI SGR text attributes, 16/256/TrueColor resolution, and CSI `m` parser.

use crate::config::{Rgb, ThemePalette};
use vte::Params;

/// Terminal color specification independent of active theme palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl TermColor {
    pub fn resolve_fg(self, palette: &ThemePalette) -> Rgb {
        match self {
            Self::Default => palette.foreground,
            Self::Indexed(idx) => indexed_color_to_rgb(idx, palette),
            Self::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }

    pub fn resolve_bg(self, palette: &ThemePalette, fallback: Rgb) -> Rgb {
        match self {
            Self::Default => fallback,
            Self::Indexed(idx) => indexed_color_to_rgb(idx, palette),
            Self::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }
}

/// Resolves an xterm 256-color index into concrete RGB using the active theme for 0..=15.
pub fn indexed_color_to_rgb(idx: u8, palette: &ThemePalette) -> Rgb {
    match idx {
        0..=15 => palette.ansi[idx as usize],
        16..=231 => {
            let n = idx - 16;
            let r = n / 36;
            let g = (n % 36) / 6;
            let b = n % 6;
            let map = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb::new(map(r), map(g), map(b))
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            Rgb::new(v, v, v)
        }
    }
}

/// Text attributes parsed from ANSI SGR (`ESC [...m`) sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl TextStyle {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Applies ANSI SGR parameters (supporting 16-color, 256-color, TrueColor semicolon & colon forms).
pub fn apply_sgr_params(style: &mut TextStyle, params: &Params) {
    if params.is_empty() {
        style.reset();
        return;
    }

    let mut iter = params.iter();
    while let Some(sub) = iter.next() {
        let code = sub.first().copied().unwrap_or(0);
        match code {
            0 => style.reset(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            7 => style.inverse = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            27 => style.inverse = false,
            30..=37 => style.fg = TermColor::Indexed((code - 30) as u8),
            38 => {
                if let Some(color) = parse_extended_color(sub, &mut iter) {
                    style.fg = color;
                }
            }
            39 => style.fg = TermColor::Default,
            40..=47 => style.bg = TermColor::Indexed((code - 40) as u8),
            48 => {
                if let Some(color) = parse_extended_color(sub, &mut iter) {
                    style.bg = color;
                }
            }
            49 => style.bg = TermColor::Default,
            90..=97 => style.fg = TermColor::Indexed((code - 90 + 8) as u8),
            100..=107 => style.bg = TermColor::Indexed((code - 100 + 8) as u8),
            _ => {}
        }
    }
}

fn parse_extended_color<'a, I>(sub: &[u16], iter: &mut I) -> Option<TermColor>
where
    I: Iterator<Item = &'a [u16]>,
{
    if sub.len() >= 2 {
        match sub[1] {
            5 if sub.len() >= 3 => {
                return Some(TermColor::Indexed(sub[2].min(255) as u8));
            }
            2 if sub.len() >= 6 => {
                return Some(TermColor::Rgb(
                    sub[3].min(255) as u8,
                    sub[4].min(255) as u8,
                    sub[5].min(255) as u8,
                ));
            }
            2 if sub.len() == 5 => {
                return Some(TermColor::Rgb(
                    sub[2].min(255) as u8,
                    sub[3].min(255) as u8,
                    sub[4].min(255) as u8,
                ));
            }
            _ => {}
        }
    }

    let mode = iter.next()?.first().copied()?;
    match mode {
        5 => {
            let idx = iter.next()?.first().copied()?;
            Some(TermColor::Indexed(idx.min(255) as u8))
        }
        2 => {
            let r = iter.next()?.first().copied()?.min(255) as u8;
            let g = iter.next()?.first().copied()?.min(255) as u8;
            let b = iter.next()?.first().copied()?.min(255) as u8;
            Some(TermColor::Rgb(r, g, b))
        }
        _ => None,
    }
}
