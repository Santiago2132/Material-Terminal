//! Material Design 3 CommandBlock card renderer with status accent bar,
//! duration/Git/directory pills, accessible action buttons, and ANSI TrueColor output spans.

use crate::blocks::{CommandBlock, CommandStatus, OutputLine};
use crate::terminal::TextStyle;
use crate::ui::input_editor::compact_home_path;
use crate::ui::theme::MaterialTheme;
use egui::text::LayoutJob;
use egui::{
    Button, Color32, CornerRadius, FontId, Frame, Label, Margin, Pos2, Rect, RichText, Sense,
    Stroke, TextFormat, Ui,
};

/// User-initiated actions triggered from a CommandBlock card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockAction {
    None,
    CopyCommand(String),
    CopyOutput(String),
    RerunCommand(String),
    ToggleCollapse(u64),
}

/// Renders one `CommandBlock` card and returns any action clicked by the user.
pub fn render_block_card(
    ui: &mut Ui,
    block: &CommandBlock,
    theme: &MaterialTheme,
    font_size: f32,
    line_height: f32,
    search_query: &str,
    is_active_search_block: bool,
) -> BlockAction {
    let mut action = BlockAction::None;

    let (status_label, status_color) = match block.status {
        CommandStatus::Running => ("RUN".to_string(), theme.info),
        CommandStatus::Success(_) => ("OK".to_string(), theme.success),
        CommandStatus::Error(code) => (format!("ERR {code}"), theme.error),
    };

    let border_color = if is_active_search_block {
        theme.warning
    } else if block.status.is_running() {
        theme.border_focused
    } else {
        theme.border
    };

    let card_frame = Frame::new()
        .fill(theme.surface_elevated)
        .stroke(Stroke::new(
            if is_active_search_block { 1.5 } else { 1.0 },
            border_color,
        ))
        .corner_radius(CornerRadius::same(theme.corner_radius.round() as u8))
        .inner_margin(Margin {
            left: 16,
            right: 12,
            top: 10,
            bottom: 10,
        });

    let card_resp = card_frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Row 1: Status badge + path + git branch + duration
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);

            Frame::new()
                .fill(Color32::from_rgba_unmultiplied(
                    status_color.r(),
                    status_color.g(),
                    status_color.b(),
                    34,
                ))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(status_label)
                            .small()
                            .strong()
                            .color(status_color),
                    );
                });

            ui.label(
                RichText::new(compact_home_path(&block.working_dir)).color(theme.foreground_muted),
            );

            if let Some(branch) = &block.git_branch {
                Frame::new()
                    .fill(theme.surface_highest)
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("git:{branch}"))
                                .small()
                                .color(theme.accent),
                        );
                    });
            }

            Frame::new()
                .fill(theme.surface)
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(block.formatted_duration())
                            .small()
                            .color(theme.foreground_muted),
                    );
                });
        });

        // Row 2: Command text + always-accessible action toolbar (wraps cleanly in narrow splits)
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

            if ui
                .add(
                    Button::new(RichText::new("Re-run").strong().color(theme.accent))
                        .fill(theme.accent_soft)
                        .corner_radius(CornerRadius::same(6)),
                )
                .clicked()
            {
                action = BlockAction::RerunCommand(block.command_text.clone());
            }

            if ui
                .add(
                    Button::new(RichText::new("Copy Cmd").color(theme.foreground))
                        .fill(theme.surface)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(6)),
                )
                .clicked()
            {
                action = BlockAction::CopyCommand(block.command_text.clone());
            }

            if block.has_output()
                && ui
                    .add(
                        Button::new(RichText::new("Copy Out").color(theme.foreground))
                            .fill(theme.surface)
                            .stroke(Stroke::new(1.0, theme.border))
                            .corner_radius(CornerRadius::same(6)),
                    )
                    .clicked()
            {
                action = BlockAction::CopyOutput(block.plain_output_text());
            }

            let collapse_label = if block.is_collapsed {
                "Expand"
            } else {
                "Collapse"
            };
            if ui
                .add(
                    Button::new(RichText::new(collapse_label).color(theme.foreground))
                        .fill(theme.surface_highest)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(6)),
                )
                .clicked()
            {
                action = BlockAction::ToggleCollapse(block.id);
            }
        });

        // Command text line
        ui.add_space(6.0);
        let cmd_job =
            build_highlighted_command_job(&block.command_text, font_size, theme, search_query);
        ui.add(Label::new(cmd_job).selectable(true));

        // Output body
        if block.is_collapsed {
            let line_count = block.visible_lines().len();
            if line_count > 0 {
                ui.add_space(6.0);
                if ui
                    .add(
                        Label::new(
                            RichText::new(format!(
                                "--- {line_count} output lines collapsed (click Expand to view) ---"
                            ))
                            .color(theme.foreground_subtle),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    action = BlockAction::ToggleCollapse(block.id);
                }
            }
        } else {
            let visible = block.visible_lines();
            if !visible.is_empty() {
                ui.add_space(6.0);
                let out_job =
                    build_output_layout_job(visible, theme, font_size, line_height, search_query);
                ui.add(Label::new(out_job).selectable(true));
            }
        }
    });

    // Paint crisp Material left status strip inside the card edge
    let rect = card_resp.response.rect;
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 3.0, rect.top() + 6.0),
        Pos2::new(rect.left() + 6.5, rect.bottom() - 6.0),
    );
    ui.painter()
        .rect_filled(strip_rect, CornerRadius::same(2), status_color);

    action
}

fn build_highlighted_command_job(
    command: &str,
    font_size: f32,
    theme: &MaterialTheme,
    search_query: &str,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let prompt_fmt = TextFormat {
        font_id: FontId::monospace(font_size),
        color: theme.accent,
        ..Default::default()
    };
    job.append("$ ", 0.0, prompt_fmt);

    let base_fmt = TextFormat {
        font_id: FontId::monospace(font_size),
        color: theme.foreground,
        ..Default::default()
    };
    append_with_search_highlights(&mut job, command, &base_fmt, search_query, theme);
    job
}

fn build_output_layout_job(
    lines: &[OutputLine],
    theme: &MaterialTheme,
    font_size: f32,
    line_height: f32,
    search_query: &str,
) -> LayoutJob {
    let mut job = LayoutJob::default();

    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            let nl_fmt = TextFormat {
                font_id: FontId::monospace(font_size),
                line_height: Some((font_size * line_height).round()),
                ..Default::default()
            };
            job.append("\n", 0.0, nl_fmt);
        }

        let spans = line.spans();
        if spans.is_empty() {
            let empty_fmt = theme.text_format_for(TextStyle::default(), font_size);
            job.append("", 0.0, empty_fmt);
            continue;
        }

        for span in spans {
            let mut fmt = theme.text_format_for(span.style, font_size);
            fmt.line_height = Some((font_size * line_height).round());
            append_with_search_highlights(&mut job, &span.text, &fmt, search_query, theme);
        }
    }

    job
}

fn append_with_search_highlights(
    job: &mut LayoutJob,
    text: &str,
    base_fmt: &TextFormat,
    search_query: &str,
    theme: &MaterialTheme,
) {
    let query = search_query.trim();
    if query.is_empty() {
        job.append(text, 0.0, base_fmt.clone());
        return;
    }

    let lower_text = text.to_ascii_lowercase();
    let lower_query = query.to_ascii_lowercase();
    if lower_text.len() != text.len() {
        job.append(text, 0.0, base_fmt.clone());
        return;
    }

    let mut pos = 0;
    while let Some(rel_idx) = lower_text[pos..].find(&lower_query) {
        let match_start = pos + rel_idx;
        let match_end = match_start + lower_query.len();
        if match_start > pos {
            job.append(&text[pos..match_start], 0.0, base_fmt.clone());
        }
        let mut hl_fmt = base_fmt.clone();
        hl_fmt.background = theme.warning;
        hl_fmt.color = Color32::from_rgb(20, 22, 30);
        job.append(&text[match_start..match_end], 0.0, hl_fmt);
        pos = match_end;
    }
    if pos < text.len() {
        job.append(&text[pos..], 0.0, base_fmt.clone());
    }
}
