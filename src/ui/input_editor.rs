//! Anchored modern multiline command input editor with Material 3 elevation,
//! history navigation, ghost-text SQLite/path autocomplete, selection, and word editing.

use crate::history::HistoryStore;
use crate::ui::theme::MaterialTheme;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, CornerRadius, EventFilter, FontId, Frame, Id, Key, KeyboardShortcut, Layout,
    Margin, Modifiers, RichText, Stroke, TextEdit, Ui,
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

/// Persistent per-pane state for the anchored bottom command editor.
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
    /// Replaces buffer with a command (e.g. from "Re-run" or Command Palette) and focuses input.
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

    /// Renders the Material 3 bottom input dock and returns any submitted action.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        pane_id: u64,
        theme: &MaterialTheme,
        font_size: f32,
        working_dir: &Path,
        git_branch: Option<&str>,
        is_running: bool,
        history: Option<&HistoryStore>,
        recent_history: &[String],
        shortcuts_blocked: bool,
    ) -> InputAction {
        let mut action = InputAction::None;
        let editor_id = Id::NULL.with(("material_input_editor", pane_id));

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
            theme.border_focused
        } else {
            theme.border
        };

        let card_frame = Frame::new()
            .fill(theme.surface_elevated)
            .stroke(Stroke::new(if is_focused { 1.4 } else { 1.0 }, border_color))
            .corner_radius(CornerRadius::same(theme.corner_radius.round() as u8))
            .inner_margin(Margin::symmetric(12, 9));

        card_frame.show(ui, |ui| {
            // Context header line: formatted directory + Git branch + mode hint
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(compact_home_path(working_dir))
                        .small()
                        .strong()
                        .color(theme.accent),
                );
                if let Some(branch) = git_branch {
                    Frame::new()
                        .fill(theme.accent_soft)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("git:{branch}"))
                                    .small()
                                    .color(theme.foreground),
                            );
                        });
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if is_running {
                        if ui
                            .small_button(RichText::new("Stop (^C)").color(theme.error))
                            .clicked()
                        {
                            action = InputAction::SendInterrupt;
                        }
                        ui.label(
                            RichText::new("Running · stdin active")
                                .small()
                                .color(theme.warning),
                        );
                    } else if self.ghost_suffix.is_some() && cursor_at_end {
                        ui.label(
                            RichText::new("Tab to complete · Shift+Enter newline")
                                .small()
                                .color(theme.foreground_subtle),
                        );
                    } else {
                        ui.label(
                            RichText::new("Enter run · Shift+Enter newline")
                                .small()
                                .color(theme.foreground_subtle),
                        );
                    }
                });
            });

            ui.add_space(4.0);

            // Prompt glyph + multiline TextEdit
            ui.horizontal_top(|ui| {
                let prompt_glyph = if is_running { "▶" } else { "❯" };
                let prompt_color = if is_running {
                    theme.warning
                } else {
                    theme.accent
                };
                ui.label(
                    RichText::new(prompt_glyph)
                        .font(FontId::monospace(font_size))
                        .strong()
                        .color(prompt_color),
                );

                let hint = if is_running {
                    "Send line to running process stdin..."
                } else {
                    "Enter a command..."
                };

                let event_filter = EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                };

                let text_edit = TextEdit::multiline(&mut self.buffer)
                    .id(editor_id)
                    .desired_width(f32::INFINITY)
                    .desired_rows(1)
                    .frame(Frame::NONE)
                    .font(FontId::monospace(font_size))
                    .text_color(theme.foreground)
                    .hint_text(RichText::new(hint).color(theme.foreground_subtle))
                    .return_key(Some(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)))
                    .event_filter(event_filter);

                let mut output = text_edit.show(ui);

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

                // Render ghost autocomplete text directly after the cursor at end of line.
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
                        FontId::monospace(font_size),
                        theme.foreground_subtle,
                    );
                }
            });
        });

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
