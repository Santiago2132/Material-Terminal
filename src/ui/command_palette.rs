//! Fuzzy-searchable Command Palette (`Cmd+K` / `Ctrl+Shift+P`) combining
//! workspace actions, live theme switching, and SQLite command history.

use crate::config::BuiltinTheme;
use crate::history::HistoryEntry;
use crate::ui::theme::MaterialTheme;
use egui::{
    Align, Area, Color32, Context, CornerRadius, Frame, Id, Key, Layout, Margin, Modifiers, Order,
    Pos2, RichText, ScrollArea, Sense, Stroke, TextEdit,
};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// Actions executable from the Command Palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    SetTheme(BuiltinTheme),
    SplitVertical,
    SplitHorizontal,
    NewTab,
    ClosePane,
    ClearBlocks,
    ToggleSearch,
    CollapseAllBlocks,
    ExpandAllBlocks,
    OpenConfig,
    RunHistoryCommand(String),
}

#[derive(Debug, Clone)]
struct PaletteItem {
    title: String,
    subtitle: String,
    shortcut: String,
    category: &'static str,
    action: PaletteAction,
}

/// Persistent UI state for the modal Command Palette.
#[derive(Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
    just_opened: bool,
    matcher: SkimMatcherV2,
}

impl CommandPaletteState {
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
            self.query.clear();
            self.selected_idx = 0;
            self.just_opened = true;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected_idx = 0;
        self.just_opened = false;
    }

    fn build_items(
        &self,
        active_theme: BuiltinTheme,
        history: &[HistoryEntry],
    ) -> Vec<PaletteItem> {
        let mut items = vec![
            PaletteItem {
                title: "Split Pane Vertically (Side by Side)".to_string(),
                subtitle: "Open a new PTY pane to the right".to_string(),
                shortcut: "Cmd+D".to_string(),
                category: "Pane",
                action: PaletteAction::SplitVertical,
            },
            PaletteItem {
                title: "Split Pane Horizontally (Top / Bottom)".to_string(),
                subtitle: "Open a new PTY pane below".to_string(),
                shortcut: "Cmd+Shift+D".to_string(),
                category: "Pane",
                action: PaletteAction::SplitHorizontal,
            },
            PaletteItem {
                title: "New Tab".to_string(),
                subtitle: "Create a new workspace tab".to_string(),
                shortcut: "Cmd+T".to_string(),
                category: "Tab",
                action: PaletteAction::NewTab,
            },
            PaletteItem {
                title: "Close Active Pane / Tab".to_string(),
                subtitle: "Terminate active shell pane".to_string(),
                shortcut: "Cmd+W".to_string(),
                category: "Tab",
                action: PaletteAction::ClosePane,
            },
            PaletteItem {
                title: "Clear Command Blocks".to_string(),
                subtitle: "Clear completed blocks in active pane".to_string(),
                shortcut: "Cmd+L".to_string(),
                category: "View",
                action: PaletteAction::ClearBlocks,
            },
            PaletteItem {
                title: "Search Blocks".to_string(),
                subtitle: "Find text across commands and output".to_string(),
                shortcut: "Cmd+F".to_string(),
                category: "View",
                action: PaletteAction::ToggleSearch,
            },
            PaletteItem {
                title: "Collapse All Blocks".to_string(),
                subtitle: "Fold output cards in active pane".to_string(),
                shortcut: String::new(),
                category: "View",
                action: PaletteAction::CollapseAllBlocks,
            },
            PaletteItem {
                title: "Expand All Blocks".to_string(),
                subtitle: "Unfold output cards in active pane".to_string(),
                shortcut: String::new(),
                category: "View",
                action: PaletteAction::ExpandAllBlocks,
            },
            PaletteItem {
                title: "Open config.toml".to_string(),
                subtitle: "Edit ~/.config/material-terminal/config.toml (live hot-reload)"
                    .to_string(),
                shortcut: "Cmd+,".to_string(),
                category: "Config",
                action: PaletteAction::OpenConfig,
            },
        ];

        for theme in BuiltinTheme::ALL {
            let active_badge = if theme == active_theme {
                " (active)"
            } else {
                ""
            };
            items.push(PaletteItem {
                title: format!("Theme: {}{}", theme.display_name(), active_badge),
                subtitle: format!("Switch Material palette to {}", theme.id()),
                shortcut: String::new(),
                category: "Theme",
                action: PaletteAction::SetTheme(theme),
            });
        }

        for entry in history.iter().take(120) {
            let status_txt = match entry.exit_code {
                Some(0) => "exit 0".to_string(),
                Some(c) => format!("exit {c}"),
                None => "history".to_string(),
            };
            items.push(PaletteItem {
                title: entry.command.clone(),
                subtitle: format!("{} · {}", entry.working_dir, status_txt),
                shortcut: String::new(),
                category: "History",
                action: PaletteAction::RunHistoryCommand(entry.command.clone()),
            });
        }

        if self.query.trim().is_empty() {
            return items;
        }

        let q = self.query.trim();
        let mut scored: Vec<(i64, PaletteItem)> = items
            .into_iter()
            .filter_map(|item| {
                let search_haystack = format!("{} {} {}", item.category, item.title, item.subtitle);
                self.matcher
                    .fuzzy_match(&search_haystack, q)
                    .map(|score| (score, item))
            })
            .collect();
        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        scored.into_iter().map(|(_, item)| item).collect()
    }

    /// Renders the Command Palette modal if open and returns a triggered action.
    pub fn show(
        &mut self,
        ctx: &Context,
        theme: &MaterialTheme,
        active_theme: BuiltinTheme,
        history: &[HistoryEntry],
    ) -> Option<PaletteAction> {
        if !self.open {
            return None;
        }

        let mut chosen: Option<PaletteAction> = None;
        let items = self.build_items(active_theme, history);
        if !items.is_empty() {
            self.selected_idx = self.selected_idx.min(items.len() - 1);
        } else {
            self.selected_idx = 0;
        }

        // Keyboard navigation inside modal
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.close();
            return None;
        }
        if !items.is_empty() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                self.selected_idx = (self.selected_idx + 1) % items.len();
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                self.selected_idx = if self.selected_idx == 0 {
                    items.len() - 1
                } else {
                    self.selected_idx - 1
                };
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                chosen = Some(items[self.selected_idx].action.clone());
                self.close();
                return chosen;
            }
        }

        let screen = ctx.content_rect();
        // Subtle dimmed backdrop
        Area::new(Id::NULL.with("command_palette_backdrop"))
            .order(Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(120));
                if resp.clicked() {
                    self.close();
                }
            });

        if !self.open {
            return None;
        }

        let modal_width = (screen.width() * 0.62).clamp(380.0, 660.0);
        let modal_pos = Pos2::new(screen.center().x - modal_width * 0.5, screen.top() + 58.0);

        Area::new(Id::NULL.with("command_palette_modal"))
            .order(Order::Foreground)
            .fixed_pos(modal_pos)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme.surface_elevated)
                    .stroke(Stroke::new(1.2, theme.border_focused))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(modal_width - 24.0);

                        let input_id = Id::NULL.with("command_palette_input");
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.query)
                                .id(input_id)
                                .desired_width(f32::INFINITY)
                                .hint_text(
                                    RichText::new("Type a command, theme, or history query...")
                                        .color(theme.foreground_subtle),
                                ),
                        );
                        if self.just_opened {
                            resp.request_focus();
                            self.just_opened = false;
                        }
                        if resp.changed() {
                            self.selected_idx = 0;
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);

                        if items.is_empty() {
                            ui.label(
                                RichText::new("No matching commands or history entries")
                                    .color(theme.foreground_muted),
                            );
                            return;
                        }

                        ScrollArea::vertical()
                            .max_height((screen.height() * 0.52).max(200.0))
                            .show(ui, |ui| {
                                for (idx, item) in items.iter().take(40).enumerate() {
                                    let is_selected = idx == self.selected_idx;
                                    let row_fill = if is_selected {
                                        theme.accent_soft
                                    } else {
                                        Color32::TRANSPARENT
                                    };
                                    let row_stroke = if is_selected {
                                        Stroke::new(1.0, theme.border_focused)
                                    } else {
                                        Stroke::NONE
                                    };

                                    let row_resp = Frame::new()
                                        .fill(row_fill)
                                        .stroke(row_stroke)
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                Frame::new()
                                                    .fill(theme.surface_highest)
                                                    .corner_radius(CornerRadius::same(5))
                                                    .inner_margin(Margin::symmetric(6, 2))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(item.category)
                                                                .small()
                                                                .color(theme.accent),
                                                        );
                                                    });
                                                ui.vertical(|ui| {
                                                    ui.label(
                                                        RichText::new(&item.title)
                                                            .strong()
                                                            .color(theme.foreground),
                                                    );
                                                    if !item.subtitle.is_empty() {
                                                        ui.label(
                                                            RichText::new(&item.subtitle)
                                                                .small()
                                                                .color(theme.foreground_muted),
                                                        );
                                                    }
                                                });
                                                if !item.shortcut.is_empty() {
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            ui.label(
                                                                RichText::new(&item.shortcut)
                                                                    .small()
                                                                    .monospace()
                                                                    .color(theme.foreground_subtle),
                                                            );
                                                        },
                                                    );
                                                }
                                            });
                                        })
                                        .response
                                        .interact(Sense::click());

                                    if is_selected {
                                        row_resp.scroll_to_me(Some(Align::Center));
                                    }
                                    if row_resp.hovered() {
                                        self.selected_idx = idx;
                                    }
                                    if row_resp.clicked() {
                                        chosen = Some(item.action.clone());
                                    }
                                }
                            });
                    });
            });

        if chosen.is_some() {
            self.close();
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_search_ranks_theme_and_history_matches() {
        let mut palette = CommandPaletteState {
            query: "catppuccin".to_string(),
            ..Default::default()
        };
        let items = palette.build_items(BuiltinTheme::TokyoNight, &[]);
        assert!(!items.is_empty());
        assert_eq!(
            items[0].action,
            PaletteAction::SetTheme(BuiltinTheme::CatppuccinMocha)
        );

        palette.query = "cargobld".to_string();
        let hist = vec![HistoryEntry {
            id: 1,
            command: "cargo build --release".to_string(),
            working_dir: "/tmp".to_string(),
            exit_code: Some(0),
            timestamp_ms: 100,
        }];
        let items = palette.build_items(BuiltinTheme::TokyoNight, &hist);
        assert_eq!(
            items[0].action,
            PaletteAction::RunHistoryCommand("cargo build --release".to_string())
        );
    }
}
