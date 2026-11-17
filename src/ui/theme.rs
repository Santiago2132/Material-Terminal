//! Material Design 3 surface derivation and egui Visuals synchronization
//! driven by the active `AppConfig` and `ThemePalette`.

use crate::config::{AppConfig, Rgb, ThemePalette};
use crate::terminal::{TermColor, TextStyle};
use egui::epaint::Shadow;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    TextFormat, TextStyle as EguiTextStyle, Visuals,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Resolved `Color32` Material Design 3 tokens derived from `ThemePalette` + `opacity`.
#[derive(Debug, Clone, Copy)]
pub struct MaterialTheme {
    pub palette: ThemePalette,
    pub opacity: f32,
    pub corner_radius: f32,
    pub background: Color32,
    pub surface: Color32,
    pub surface_elevated: Color32,
    pub surface_highest: Color32,
    pub border: Color32,
    pub border_focused: Color32,
    pub foreground: Color32,
    pub foreground_muted: Color32,
    pub foreground_subtle: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub selection_bg: Color32,
}

impl MaterialTheme {
    pub fn from_config(cfg: &AppConfig) -> Self {
        let palette = cfg.palette();
        let alpha = 255u8;
        let card_alpha = 255u8;

        Self {
            palette,
            opacity: cfg.window.opacity.clamp(0.25, 1.0),
            corner_radius: cfg.window.corner_radius,
            background: rgb_alpha(palette.background, alpha),
            surface: rgb_alpha(palette.surface, card_alpha),
            surface_elevated: rgb_alpha(palette.surface_elevated, card_alpha),
            surface_highest: rgb_alpha(palette.surface_highest, 255),
            border: rgb_opaque(palette.border),
            border_focused: rgb_opaque(palette.border_focused),
            foreground: rgb_opaque(palette.foreground),
            foreground_muted: rgb_opaque(palette.foreground_muted),
            foreground_subtle: rgb_opaque(palette.foreground_subtle),
            accent: rgb_opaque(palette.accent),
            accent_soft: rgb_opaque(palette.accent_soft),
            success: rgb_opaque(palette.success),
            error: rgb_opaque(palette.error),
            warning: rgb_opaque(palette.warning),
            info: rgb_opaque(palette.info),
            selection_bg: rgb_alpha(palette.selection_bg, 200),
        }
    }

    pub fn mono_font(&self, size: f32) -> FontId {
        FontId::monospace(size)
    }

    /// Resolves a terminal `TextStyle` into an `egui::TextFormat` for ANSI span rendering.
    pub fn text_format_for(&self, style: TextStyle, font_size: f32) -> TextFormat {
        let mut fg = style.fg.resolve_fg(&self.palette);
        let mut bg_opt = match style.bg {
            TermColor::Default => None,
            other => Some(other.resolve_bg(&self.palette, self.palette.surface_elevated)),
        };

        if style.inverse {
            let old_fg = fg;
            fg = bg_opt.unwrap_or(self.palette.surface_elevated);
            bg_opt = Some(old_fg);
        }

        let mut fg_c32 = rgb_opaque(fg);
        if style.dim {
            fg_c32 = Color32::from_rgba_unmultiplied(fg_c32.r(), fg_c32.g(), fg_c32.b(), 155);
        } else if style.bold {
            // Subtle luminance boost for bold terminal output
            fg_c32 = Color32::from_rgb(
                fg_c32.r().saturating_add(16),
                fg_c32.g().saturating_add(16),
                fg_c32.b().saturating_add(16),
            );
        }

        let background = bg_opt.map(rgb_opaque).unwrap_or(Color32::TRANSPARENT);
        let underline = if style.underline {
            Stroke::new(1.0, fg_c32)
        } else {
            Stroke::NONE
        };

        TextFormat {
            font_id: FontId::monospace(font_size),
            color: fg_c32,
            background,
            italics: style.italic,
            underline,
            ..Default::default()
        }
    }

    /// Applies Material 3 theme colors, typography sizes, and rounded widgets to `egui::Context`.
    pub fn apply_to_egui(&self, ctx: &Context, cfg: &AppConfig) {
        let mut visuals = Visuals::dark();
        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface_elevated;
        visuals.extreme_bg_color = self.surface;
        visuals.faint_bg_color = self.surface_elevated;
        visuals.code_bg_color = self.surface_elevated;

        visuals.selection.bg_fill = self.selection_bg;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        visuals.widgets.noninteractive.bg_fill = self.surface;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.foreground);
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

        visuals.widgets.inactive.bg_fill = self.surface_elevated;
        visuals.widgets.inactive.weak_bg_fill = self.surface_elevated;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.foreground_muted);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

        visuals.widgets.hovered.bg_fill = self.surface_highest;
        visuals.widgets.hovered.weak_bg_fill = self.surface_highest;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border_focused);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.foreground);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

        visuals.widgets.active.bg_fill = self.accent_soft;
        visuals.widgets.active.weak_bg_fill = self.accent_soft;
        visuals.widgets.active.bg_stroke = Stroke::new(1.5, self.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.foreground);
        visuals.widgets.active.corner_radius = CornerRadius::same(8);

        visuals.window_corner_radius = CornerRadius::same(self.corner_radius.round() as u8);
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(110),
        };
        visuals.popup_shadow = visuals.window_shadow;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.global_style()).clone();
        let text_styles: BTreeMap<EguiTextStyle, FontId> = [
            (EguiTextStyle::Small, FontId::proportional(12.0)),
            (EguiTextStyle::Body, FontId::proportional(13.5)),
            (EguiTextStyle::Button, FontId::proportional(13.0)),
            (EguiTextStyle::Heading, FontId::proportional(16.5)),
            (EguiTextStyle::Monospace, FontId::monospace(cfg.font.size)),
        ]
        .into();
        style.text_styles = text_styles;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.scroll.bar_width = 7.0;
        style.spacing.scroll.floating = true;
        ctx.set_global_style(style);
    }
}

pub fn rgb_opaque(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

pub fn rgb_alpha(rgb: Rgb, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb.r, rgb.g, rgb.b, alpha)
}

/// Attempts to load a system `.ttf`/`.otf` monospace font matching `family_name`
/// into `FontFamily::Monospace` while retaining built-in fallbacks.
pub fn apply_configured_font(ctx: &Context, family_name: &str) {
    let mut defs = FontDefinitions::default();
    if let Some(bytes) = find_system_font_bytes(family_name) {
        let key = "user-configured-mono".to_string();
        defs.font_data
            .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
        defs.families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, key);
    }
    ctx.set_fonts(defs);
}

fn find_system_font_bytes(family_name: &str) -> Option<Vec<u8>> {
    let normalized = family_name
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "");
    if normalized.is_empty() {
        return None;
    }

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join("Library/Fonts"));
        search_dirs.push(home.join(".local/share/fonts"));
        search_dirs.push(home.join(".fonts"));
    }
    search_dirs.push(PathBuf::from("/Library/Fonts"));
    search_dirs.push(PathBuf::from("/System/Library/Fonts"));
    search_dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
    search_dirs.push(PathBuf::from("/usr/share/fonts"));
    search_dirs.push(PathBuf::from("/usr/local/share/fonts"));

    let mut best_match: Option<(u8, PathBuf)> = None;
    for dir in search_dirs {
        collect_matching_font(&dir, &normalized, 0, &mut best_match);
    }

    let (_, path) = best_match?;
    std::fs::read(path).ok()
}

fn collect_matching_font(
    dir: &PathBuf,
    normalized_target: &str,
    depth: u8,
    best: &mut Option<(u8, PathBuf)>,
) {
    if depth > 3 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_matching_font(&path, normalized_target, depth + 1, best);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let norm_stem = stem.to_ascii_lowercase().replace([' ', '-', '_'], "");
        if !norm_stem.contains(normalized_target) {
            continue;
        }
        // Prefer regular weight over bold/italic/condensed variants.
        let score = if norm_stem == normalized_target
            || norm_stem == format!("{normalized_target}regular")
        {
            3
        } else if !norm_stem.contains("italic") && !norm_stem.contains("bold") {
            2
        } else {
            1
        };
        if best.as_ref().is_none_or(|(best_s, _)| score > *best_s) {
            *best = Some((score, path));
        }
    }
}
