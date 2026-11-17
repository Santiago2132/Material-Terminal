//! Material Design 3 CommandBlock card renderer with status accent bar,
//! duration/Git/directory pills, hover actions, and ANSI TrueColor output spans.

use crate::blocks::{CommandBlock, CommandStatus, OutputLine};
use crate::terminal::TextStyle;
use crate::ui::input_editor::compact_home_path;
use crate::ui::theme::MaterialTheme;
use egui::text::LayoutJob;
use egui::{
    Align, Color32, CornerRadius, FontId, Frame, Id, Label, Layout, Margin, Pos2, Rect, RichText,
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

    let status_color = match block.status {
        CommandStatus::Running => theme.info,
        CommandStatus::Success(_) => theme.success,
        CommandStatus::Error(_) => theme.error,
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
            left: 14,
            right: 12,
            top: 9,
            bottom: 9,
        });

    let hover_id = Id::NULL.with(("block_card_hovered", block.id));
    let was_hovered = ui.data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));

    let card_resp = card_frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        let header_hovered = was_hovered;

        // Header row: status indicator, path, git branch, command, duration & hover actions
        ui.horizontal_wrapped(|ui| {
            let status_dot = match block.status {
                CommandStatus::Running => "◉",
                CommandStatus::Success(_) => "●",
                CommandStatus::Error(_) => "▲",
            };
            ui.label(RichText::new(status_dot).small().color(status_color));

            ui.label(
                RichText::new(compact_home_path(&block.working_dir))
                    .small()
                    .color(theme.foreground_muted),
            );

            if let Some(branch) = &block.git_branch {
                Frame::new()
                    .fill(theme.surface_highest)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("git:{branch}"))
                                .small()
                                .color(theme.accent),
                        );
                    });
            }

            if let CommandStatus::Error(code) = block.status {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(
                        theme.error.r(),
                        theme.error.g(),
                        theme.error.b(),
                        36,
                    ))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("exit {code}"))
                                .small()
                                .strong()
                                .color(theme.error),
                        );
                    });
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Collapse/Expand button
                let collapse_icon = if block.is_collapsed { "▸" } else { "▾" };
                if ui
                    .small_button(RichText::new(collapse_icon).color(theme.foreground_muted))
                    .on_hover_text(if block.is_collapsed {
                        "Expand block output"
                    } else {
                        "Collapse block output"
                    })
                    .clicked()
                {
                    action = BlockAction::ToggleCollapse(block.id);
                }

                // Duration pill
                Frame::new()
                    .fill(theme.surface_highest)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(7, 1))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(block.formatted_duration())
                                .small()
                                .color(theme.foreground_muted),
                        );
                    });

                // Hover actions: Copy Command, Copy Output, Re-run
                let show_actions = header_hovered || block.status.is_running();
                if show_actions {
                    if ui
                        .small_button(RichText::new("Re-run").small().color(theme.accent))
                        .on_hover_text("Re-run command in this pane")
                        .clicked()
                    {
                        action = BlockAction::RerunCommand(block.command_text.clone());
                    }
                    if block.has_output()
                        && ui
                            .small_button(
                                RichText::new("Copy Output")
                                    .small()
                                    .color(theme.foreground_muted),
                            )
                            .on_hover_text("Copy plain text output")
                            .clicked()
                    {
                        action = BlockAction::CopyOutput(block.plain_output_text());
                    }
                    if ui
                        .small_button(
                            RichText::new("Copy Cmd")
                                .small()
                                .color(theme.foreground_muted),
                        )
                        .on_hover_text("Copy command text")
                        .clicked()
                    {
                        action = BlockAction::CopyCommand(block.command_text.clone());
                    }
                }
            });
        });

        // Command text line
        ui.add_space(3.0);
        let cmd_job =
            build_highlighted_command_job(&block.command_text, font_size, theme, search_query);
        ui.add(Label::new(cmd_job).selectable(true));

        // Output body
        if block.is_collapsed {
            let line_count = block.visible_lines().len();
            if line_count > 0 {
                ui.add_space(4.0);
                if ui
                    .add(
                        Label::new(
                            RichText::new(format!("··· {line_count} output lines collapsed ···"))
                                .small()
                                .color(theme.foreground_subtle),
                        )
                        .sense(egui::Sense::click()),
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
    let is_now_hovered = ui.rect_contains_pointer(rect);
    ui.data_mut(|d| d.insert_temp(hover_id, is_now_hovered));
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 2.0, rect.top() + 5.0),
        Pos2::new(rect.left() + 5.0, rect.bottom() - 5.0),
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
    let extra_leading = ((line_height - 1.0).max(0.0) * font_size * 0.45).round();

    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            let nl_fmt = TextFormat {
                font_id: FontId::monospace(font_size),
                line_height: Some((font_size * line_height).round() + extra_leading * 0.0),
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
        // Fallback when Unicode case-folding changes byte lengths: safe plain append.
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
