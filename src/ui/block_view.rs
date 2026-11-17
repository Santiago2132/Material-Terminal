//! Liquid Glass CommandBlock card renderer matching the design mockup:
//! subtle glass surface, 1px top specular highlight, 3px left status bar,
//! clean metadata + action strip, and ANSI TrueColor output spans.

use crate::blocks::{CommandBlock, CommandStatus, OutputLine};
use crate::terminal::TextStyle;
use crate::ui::input_editor::compact_home_path;
use crate::ui::theme::{MaterialTheme, paint_specular_highlight};
use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, FontId, Frame, Id, Label, Layout, Margin, Pos2, Rect,
    RichText, Sense, Stroke, TextFormat, Ui,
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

/// Renders one Liquid Glass `CommandBlock` card and returns any triggered action.
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

    let hover_id = Id::NULL.with(("glass_block_hover", block.id));
    let was_hovered = ui.data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));

    let fill = if was_hovered {
        theme.glass_block_hover
    } else {
        theme.glass_block
    };
    let border_color = if is_active_search_block {
        theme.warning
    } else if block.status.is_running() {
        theme.border_focused
    } else if was_hovered {
        theme.glass_border
    } else {
        theme.glass_border_subtle
    };

    let card_radius = 14.0_f32;
    let card_frame = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(
            if is_active_search_block { 1.4 } else { 1.0 },
            border_color,
        ))
        .corner_radius(CornerRadius::same(card_radius as u8))
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 11,
            bottom: 11,
        });

    let card_resp = card_frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Single clean header row: dir + git + exit badge (left) | duration + actions (right)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);

            ui.label(
                RichText::new(compact_home_path(&block.working_dir))
                    .monospace()
                    .strong()
                    .color(theme.accent),
            );

            if let Some(branch) = &block.git_branch {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 13))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("git:{branch}"))
                                .small()
                                .monospace()
                                .color(theme.foreground_muted),
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
                    .inner_margin(Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("exit {code}"))
                                .small()
                                .monospace()
                                .strong()
                                .color(theme.error),
                        );
                    });
            } else if block.status.is_running() {
                Frame::new()
                    .fill(theme.accent_soft)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("running")
                                .small()
                                .monospace()
                                .color(theme.accent),
                        );
                    });
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                let collapse_label = if block.is_collapsed {
                    "Expand"
                } else {
                    "Collapse"
                };
                if ui
                    .add(
                        Button::new(
                            RichText::new(collapse_label)
                                .small()
                                .color(theme.foreground),
                        )
                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 16))
                        .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                        .corner_radius(CornerRadius::same(6)),
                    )
                    .clicked()
                {
                    action = BlockAction::ToggleCollapse(block.id);
                }

                if ui
                    .add(
                        Button::new(RichText::new("Re-run").small().color(theme.accent))
                            .fill(theme.accent_soft)
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgba_unmultiplied(
                                    theme.accent.r(),
                                    theme.accent.g(),
                                    theme.accent.b(),
                                    80,
                                ),
                            ))
                            .corner_radius(CornerRadius::same(6)),
                    )
                    .clicked()
                {
                    action = BlockAction::RerunCommand(block.command_text.clone());
                }

                let copy_text = if block.has_output() {
                    BlockAction::CopyOutput(block.plain_output_text())
                } else {
                    BlockAction::CopyCommand(block.command_text.clone())
                };
                if ui
                    .add(
                        Button::new(
                            RichText::new(if block.has_output() {
                                "Copy"
                            } else {
                                "Copy Cmd"
                            })
                            .small()
                            .color(theme.foreground),
                        )
                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 16))
                        .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                        .corner_radius(CornerRadius::same(6)),
                    )
                    .on_hover_text("Copy output (or command if empty)")
                    .clicked()
                {
                    action = copy_text;
                }

                ui.label(
                    RichText::new(block.formatted_duration())
                        .small()
                        .monospace()
                        .color(theme.foreground_muted),
                );
            });
        });

        // Command line: `> command_text`
        ui.add_space(6.0);
        let cmd_job =
            build_highlighted_command_job(&block.command_text, font_size, theme, search_query);
        ui.add(Label::new(cmd_job).selectable(true));

        // Output body (hidden cleanly when collapsed)
        if block.is_collapsed {
            let line_count = block.visible_lines().len();
            if line_count > 0 {
                ui.add_space(4.0);
                if ui
                    .add(
                        Label::new(
                            RichText::new(format!("... {line_count} lines collapsed"))
                                .small()
                                .monospace()
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
                let out_job = build_output_layout_job(
                    visible,
                    theme,
                    (font_size - 0.5).max(11.0),
                    line_height,
                    search_query,
                );
                ui.add(Label::new(out_job).selectable(true));
            }
        }
    });

    let rect = card_resp.response.rect;
    ui.data_mut(|d| d.insert_temp(hover_id, ui.rect_contains_pointer(rect)));

    // Specular 1px top highlight inside glass card
    paint_specular_highlight(
        ui.painter(),
        rect,
        card_radius,
        Color32::from_rgba_unmultiplied(255, 255, 255, 18),
    );

    // Left 3px status accent bar matching `.cmd-block::before`
    let strip_top = (rect.top() + 12.0).min(rect.center().y);
    let strip_bottom = (rect.bottom() - 12.0).max(rect.center().y);
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left(), strip_top),
        Pos2::new(rect.left() + 3.5, strip_bottom),
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
    job.append("> ", 0.0, prompt_fmt);

    let base_fmt = TextFormat {
        font_id: FontId::monospace(font_size),
        color: Color32::WHITE,
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
