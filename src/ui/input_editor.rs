//! Floating Liquid Glass command input dock matching the design mockup:
//! soft focus glow ring, specular top edge, context strip, multiline editing,
//! history navigation, and ghost-text SQLite/path autocomplete.

use crate::history::HistoryStore;
use crate::ui::theme::{MaterialTheme, paint_specular_highlight};
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, Button, Color32, CornerRadius, EventFilter, FontId, Frame, Id, Key,
    KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea, Stroke, StrokeKind,
    TextEdit, Ui,
};
use std::path::{Path, PathBuf};

/// Actions emitted by the anchored input editor to the owning pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    None,
    SubmitCommand(String),
    SendStdinLine(String),
    SendInterrupt,
    SendEof,
}

/// Persistent per-pane state for the floating bottom command dock.
#[derive(Debug, Clone)]
pub struct InputEditorState {
    pub buffer: String,
    history_index: Option<usize>,
    draft_backup: String,
    ghost_suffix: Option<String>,
    cached_query_text: String,
    cached_query_dir: PathBuf,
    pub request_focus: bool,
    move_cursor_to_end: bool,
}

impl Default for InputEditorState {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            history_index: None,
            draft_backup: String::new(),
            ghost_suffix: None,
            cached_query_text: String::new(),
            cached_query_dir: PathBuf::new(),
            request_focus: true,
            move_cursor_to_end: false,
        }
    }
}

impl InputEditorState {
    /// Replaces buffer with a command and focuses input.
    pub fn set_text_and_focus(&mut self, text: String) {
        self.buffer = text;
        self.history_index = None;
        self.ghost_suffix = None;
        self.cached_query_text.clear();
        self.request_focus = true;
        self.move_cursor_to_end = true;
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.history_index = None;
        self.draft_backup.clear();
        self.ghost_suffix = None;
        self.cached_query_text.clear();
    }

    /// Steps upward through deduplicated recent history (most recent first).
    pub fn step_history_up(&mut self, recent: &[String]) {
        if recent.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.draft_backup = self.buffer.clone();
                self.history_index = Some(0);
                self.buffer = recent[0].clone();
            }
            Some(idx) if idx + 1 < recent.len() => {
                let next = idx + 1;
                self.history_index = Some(next);
                self.buffer = recent[next].clone();
            }
            Some(_) => {}
        }
        self.ghost_suffix = None;
        self.move_cursor_to_end = true;
    }

    /// Steps downward back toward newer commands and eventually the preserved draft.
    pub fn step_history_down(&mut self, recent: &[String]) {
        match self.history_index {
            None => {}
            Some(0) => {
                self.history_index = None;
                self.buffer = std::mem::take(&mut self.draft_backup);
                self.move_cursor_to_end = true;
            }
            Some(idx) => {
                let next = idx - 1;
                self.history_index = Some(next);
                self.buffer = recent.get(next).cloned().unwrap_or_default();
                self.move_cursor_to_end = true;
            }
        }
        self.ghost_suffix = None;
    }

    /// Accepts the currently displayed ghost-text completion if available.
    pub fn accept_ghost(&mut self) -> bool {
        if let Some(suffix) = self.ghost_suffix.take() {
            self.buffer.push_str(&suffix);
            self.cached_query_text.clear();
            self.move_cursor_to_end = true;
            true
        } else {
            false
        }
    }

    /// Refreshes cached ghost suggestion only when buffer or cwd changes.
    pub fn refresh_ghost(
        &mut self,
        history: Option<&HistoryStore>,
        working_dir: &Path,
        is_running: bool,
    ) {
        if is_running || self.buffer.is_empty() || self.buffer.contains('\n') {
            self.ghost_suffix = None;
            self.cached_query_text = self.buffer.clone();
            self.cached_query_dir = working_dir.to_path_buf();
            return;
        }

        if self.cached_query_text == self.buffer && self.cached_query_dir == working_dir {
            return;
        }

        self.cached_query_text = self.buffer.clone();
        self.cached_query_dir = working_dir.to_path_buf();
        self.ghost_suffix = history.and_then(|h| h.suggest_ghost_suffix(&self.buffer, working_dir));
    }

    /// Computes exact desired pixel height for the floating Liquid Glass input dock.
    pub fn desired_height(&self, ui: &Ui, font_size: f32, pane_inner_width: f32) -> f32 {
        let editor_font_size = (font_size + 0.5).max(13.5);
        let font_id = FontId::monospace(editor_font_size);
        let row_h = ui
            .fonts_mut(|f| f.row_height(&font_id))
            .max(editor_font_size * 1.4);
        let wrap_w = (pane_inner_width - 125.0).max(80.0);
        let text_h = if self.buffer.is_empty() {
            row_h
        } else {
            let job =
                egui::text::LayoutJob::simple(self.buffer.clone(), font_id, Color32::WHITE, wrap_w);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            galley.size().y.max(row_h)
        };
        let clamped_text_h = text_h.clamp(row_h, row_h * 6.0);
        (68.0 + clamped_text_h).ceil()
    }

    /// Renders the Liquid Glass floating input card inside an explicit top-down rect.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        pane_id: u64,
        theme: &MaterialTheme,
        font_size: f32,
        working_dir: &Path,
        git_branch: Option<&str>,
        shell_name: &str,
        is_running: bool,
        history: Option<&HistoryStore>,
        recent_history: &[String],
        shortcuts_blocked: bool,
    ) -> InputAction {
        let mut action = InputAction::None;
        let editor_id = Id::NULL.with(("material_input_editor", pane_id));
        let editor_font_size = (font_size + 0.5).max(13.5);

        self.refresh_ghost(history, working_dir, is_running);

        let is_focused = ui.ctx().memory(|m| m.has_focus(editor_id));
        let prev_state = TextEdit::load_state(ui.ctx(), editor_id);
        let cursor_at_end = is_cursor_at_end(prev_state.as_ref(), &self.buffer);
        let cursor_on_first_line = is_cursor_on_first_line(prev_state.as_ref(), &self.buffer);
        let cursor_on_last_line = is_cursor_on_last_line(prev_state.as_ref(), &self.buffer);

        // Pre-consume special keys when editor is focused and no modal overlay is open.
        if is_focused && !shortcuts_blocked {
            if is_running && ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::C)) {
                return InputAction::SendInterrupt;
            }
            if is_running && ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::D)) {
                return InputAction::SendEof;
            }

            if !is_running
                && cursor_at_end
                && self.ghost_suffix.is_some()
                && (ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab))
                    || ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight)))
            {
                self.accept_ghost();
            }

            if !is_running
                && cursor_on_first_line
                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp))
            {
                self.step_history_up(recent_history);
            } else if !is_running
                && cursor_on_last_line
                && self.history_index.is_some()
                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown))
            {
                self.step_history_down(recent_history);
            }

            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                let submitted = std::mem::take(&mut self.buffer);
                self.clear();
                self.request_focus = true;
                if is_running {
                    return InputAction::SendStdinLine(submitted);
                } else if !submitted.trim().is_empty() {
                    return InputAction::SubmitCommand(submitted);
                } else {
                    return InputAction::None;
                }
            }
        }

        let border_color = if is_running {
            theme.warning
        } else if is_focused {
            theme.accent
        } else {
            theme.glass_border
        };

        let card_radius = 16.0_f32;
        let card_frame = Frame::new()
            .fill(theme.glass_input)
            .stroke(Stroke::new(
                if is_focused || is_running { 1.4 } else { 1.0 },
                border_color,
            ))
            .corner_radius(CornerRadius::same(card_radius as u8))
            .inner_margin(Margin::symmetric(14, 10));

        let card_resp = card_frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Top context strip: directory + git branch + shell badge | OSC 133 · Metal
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                ui.label(
                    RichText::new(compact_home_path(working_dir))
                        .small()
                        .monospace()
                        .strong()
                        .color(theme.accent),
                );
                if let Some(branch) = git_branch {
                    ui.label(
                        RichText::new(format!("git:{branch}"))
                            .small()
                            .monospace()
                            .color(theme.foreground_muted),
                    );
                }

                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(251, 146, 60, 30))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(7, 1))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(shell_name)
                                .small()
                                .monospace()
                                .color(Color32::from_rgb(251, 146, 60)),
                        );
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if is_running {
                        if ui
                            .add(
                                Button::new(
                                    RichText::new("Stop (^C)")
                                        .small()
                                        .strong()
                                        .color(theme.error),
                                )
                                .fill(Color32::from_rgba_unmultiplied(
                                    theme.error.r(),
                                    theme.error.g(),
                                    theme.error.b(),
                                    30,
                                ))
                                .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            action = InputAction::SendInterrupt;
                        }
                        ui.label(
                            RichText::new("stdin active")
                                .small()
                                .monospace()
                                .color(theme.warning),
                        );
                    } else if self.ghost_suffix.is_some() && cursor_at_end {
                        ui.label(
                            RichText::new("Tab complete · Shift+Enter newline")
                                .small()
                                .monospace()
                                .color(theme.foreground_muted),
                        );
                    } else {
                        ui.label(
                            RichText::new("OSC 133 · GPU Metal")
                                .small()
                                .monospace()
                                .color(theme.foreground_subtle),
                        );
                    }
                });
            });

            ui.add_space(7.0);

            // Input row: '>' prompt + borderless multiline TextEdit + Enter keycap
            let avail_text_h = ui.available_height().max(editor_font_size * 1.6);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                let prompt_glyph = if is_running { "stdin>" } else { ">" };
                let prompt_color = if is_running {
                    theme.warning
                } else {
                    theme.accent
                };
                ui.label(
                    RichText::new(prompt_glyph)
                        .font(FontId::monospace(editor_font_size))
                        .strong()
                        .color(prompt_color),
                );

                // Reserve right-side Enter keycap width when not running
                let keycap_reserve = if is_running { 0.0 } else { 66.0 };
                let scroll_w = (ui.available_width() - keycap_reserve).max(80.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(scroll_w, avail_text_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ScrollArea::vertical()
                            .id_salt(Id::NULL.with(("input_scroll", pane_id)))
                            .max_height(avail_text_h)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let hint = if is_running {
                                    "Send input line to running process..."
                                } else {
                                    "Enter a command (Shift+Enter newline, Cmd+K commands)..."
                                };

                                let event_filter = EventFilter {
                                    tab: true,
                                    horizontal_arrows: true,
                                    vertical_arrows: true,
                                    escape: false,
                                };

                                let editor_width = ui.available_width().max(60.0);
                                let text_edit = TextEdit::multiline(&mut self.buffer)
                                    .id(editor_id)
                                    .desired_width(editor_width)
                                    .desired_rows(1)
                                    .frame(Frame::NONE)
                                    .font(FontId::monospace(editor_font_size))
                                    .text_color(Color32::WHITE)
                                    .hint_text(RichText::new(hint).color(theme.foreground_subtle))
                                    .return_key(Some(KeyboardShortcut::new(
                                        Modifiers::SHIFT,
                                        Key::Enter,
                                    )))
                                    .event_filter(event_filter);

                                let mut output = text_edit.show(ui);
                                if output.response.changed() {
                                    ui.ctx().request_repaint();
                                }

                                if self.request_focus {
                                    output.response.request_focus();
                                    self.request_focus = false;
                                }

                                if self.move_cursor_to_end {
                                    let end_cc = CCursor::new(self.buffer.chars().count());
                                    output
                                        .state
                                        .cursor
                                        .set_char_range(Some(CCursorRange::one(end_cc)));
                                    output.state.clone().store(ui.ctx(), output.response.id);
                                    output.cursor_range = Some(CCursorRange::one(end_cc));
                                    self.move_cursor_to_end = false;
                                }

                                // Ghost autocomplete suffix inline after cursor
                                if !is_running
                                    && !self.buffer.is_empty()
                                    && let Some(suffix) = &self.ghost_suffix
                                    && is_cursor_at_end_output(&output, &self.buffer)
                                {
                                    let end_cursor = CCursor::new(self.buffer.chars().count());
                                    let cursor_rect = output.galley.pos_from_cursor(end_cursor);
                                    let pos = output.galley_pos + cursor_rect.left_top().to_vec2();
                                    let painter = ui.painter_at(output.text_clip_rect);
                                    painter.text(
                                        pos,
                                        Align2::LEFT_TOP,
                                        suffix,
                                        FontId::monospace(editor_font_size),
                                        theme.foreground_subtle,
                                    );
                                }
                            });
                    },
                );

                if !is_running {
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if ui
                            .add(
                                Button::new(
                                    RichText::new("Enter")
                                        .small()
                                        .monospace()
                                        .color(theme.foreground_muted),
                                )
                                .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 14))
                                .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                                .corner_radius(CornerRadius::same(6)),
                            )
                            .clicked()
                        {
                            let submitted = std::mem::take(&mut self.buffer);
                            self.clear();
                            self.request_focus = true;
                            if !submitted.trim().is_empty() {
                                action = InputAction::SubmitCommand(submitted);
                            }
                        }
                    });
                }
            });
        });

        let rect = card_resp.response.rect;
        paint_specular_highlight(ui.painter(), rect, card_radius, theme.glass_highlight);

        // Soft outer glow ring matching CSS `box-shadow: 0 0 0 3px var(--accent-soft)` when focused
        if is_focused && !is_running {
            ui.painter().rect_stroke(
                rect.expand(2.5),
                CornerRadius::same((card_radius + 2.5) as u8),
                Stroke::new(3.0, theme.accent_glow),
                StrokeKind::Outside,
            );
        }

        action
    }
}

fn is_cursor_at_end(state: Option<&egui::text_edit::TextEditState>, buf: &str) -> bool {
    let char_len = buf.chars().count();
    let Some(st) = state else {
        return true;
    };
    let Some(range) = st.cursor.char_range() else {
        return true;
    };
    range.primary.index.0 == char_len && range.secondary.index.0 == char_len
}

fn is_cursor_at_end_output(output: &egui::text_edit::TextEditOutput, buf: &str) -> bool {
    let char_len = buf.chars().count();
    let Some(range) = output.cursor_range else {
        return false;
    };
    range.primary.index.0 == char_len && range.secondary.index.0 == char_len
}

fn is_cursor_on_first_line(state: Option<&egui::text_edit::TextEditState>, buf: &str) -> bool {
    if !buf.contains('\n') {
        return true;
    }
    let Some(st) = state else {
        return true;
    };
    let Some(range) = st.cursor.char_range() else {
        return true;
    };
    let idx = range.primary.index.0;
    !buf.chars().take(idx).any(|c| c == '\n')
}

fn is_cursor_on_last_line(state: Option<&egui::text_edit::TextEditState>, buf: &str) -> bool {
    if !buf.contains('\n') {
        return true;
    }
    let Some(st) = state else {
        return true;
    };
    let Some(range) = st.cursor.char_range() else {
        return true;
    };
    let idx = range.primary.index.0;
    !buf.chars().skip(idx).any(|c| c == '\n')
}

pub fn compact_home_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        if stripped.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_navigation_preserves_draft_and_accepts_ghost() {
        let mut editor = InputEditorState {
            buffer: "git st".to_string(),
            ..Default::default()
        };
        let history = vec!["cargo build".to_string(), "ls -la".to_string()];

        editor.step_history_up(&history);
        assert_eq!(editor.buffer, "cargo build");
        editor.step_history_up(&history);
        assert_eq!(editor.buffer, "ls -la");
        editor.step_history_down(&history);
        assert_eq!(editor.buffer, "cargo build");
        editor.step_history_down(&history);
        assert_eq!(editor.buffer, "git st");

        editor.ghost_suffix = Some("atus".to_string());
        assert!(editor.accept_ghost());
        assert_eq!(editor.buffer, "git status");
    }
}
