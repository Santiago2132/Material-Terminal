//! Workspace UI layout: clean top tab bar, deterministic non-overlapping split panes,
//! Block Mode scroll view, Alternate Screen GPU grid, and floating search overlay.

pub mod block_view;
pub mod command_palette;
pub mod input_editor;
pub mod theme;

use crate::app::{MaterialTerminalApp, PaneNode, PaneState, SplitAxis};
use crate::blocks::CommandBlock;
use crate::terminal::{TermColor, encode_egui_key};
use crate::ui::block_view::{BlockAction, render_block_card};
use crate::ui::input_editor::{InputAction, compact_home_path};
use crate::ui::theme::{MaterialTheme, rgb_opaque};
use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Layout,
    Margin, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder,
    Vec2,
};

/// Clean top tab strip with readable tabs, dedicated close buttons, and workspace controls.
pub fn render_top_bar(ui: &mut Ui, app: &mut MaterialTerminalApp, theme: &MaterialTheme) {
    let bar_frame = Frame::new()
        .fill(theme.surface)
        .inner_margin(Margin::symmetric(12, 8));

    let bar_resp = bar_frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let theme_label = app.config.resolved_theme().display_name();
            if ui
                .add(
                    Button::new(RichText::new(theme_label).strong().color(theme.accent))
                        .fill(theme.accent_soft)
                        .stroke(Stroke::new(1.0, theme.border_focused))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Switch Theme or Open Commands (Cmd+K)")
                .clicked()
            {
                app.palette.toggle();
            }

            if ui
                .add(
                    Button::new(RichText::new("Commands").color(theme.foreground))
                        .fill(theme.surface_elevated)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Command Palette (Cmd+K / Ctrl+Shift+P)")
                .clicked()
            {
                app.palette.toggle();
            }

            if ui
                .add(
                    Button::new(RichText::new("Find").color(theme.foreground))
                        .fill(theme.surface_elevated)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Search Blocks (Cmd+F)")
                .clicked()
            {
                app.toggle_search();
            }

            if ui
                .add(
                    Button::new(RichText::new("Split -").color(theme.foreground))
                        .fill(theme.surface_elevated)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Split Horizontally Top/Bottom (Cmd+Shift+D)")
                .clicked()
            {
                app.split_focused_pane(SplitAxis::Horizontal);
            }

            if ui
                .add(
                    Button::new(RichText::new("Split |").color(theme.foreground))
                        .fill(theme.surface_elevated)
                        .stroke(Stroke::new(1.0, theme.border))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Split Vertically Side-by-Side (Cmd+D)")
                .clicked()
            {
                app.split_focused_pane(SplitAxis::Vertical);
            }

            // Remaining space on the left hosts scrollable tabs + '+ Tab' button cleanly.
            let tabs_avail_w = ui.available_width().max(80.0);
            ui.allocate_ui_with_layout(
                Vec2::new(tabs_avail_w, 30.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ScrollArea::horizontal()
                        .id_salt(Id::NULL.with("top_tabs_scroll"))
                        .max_width(tabs_avail_w)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;

                                let mut switch_to: Option<usize> = None;
                                let mut close_tab_idx: Option<usize> = None;

                                for (idx, tab) in app.tabs.iter().enumerate() {
                                    let is_active = idx == app.active_tab;
                                    let title = app.tab_title(tab);

                                    let fill = if is_active {
                                        theme.surface_elevated
                                    } else {
                                        theme.surface
                                    };
                                    let stroke = if is_active {
                                        Stroke::new(1.2, theme.border_focused)
                                    } else {
                                        Stroke::new(1.0, theme.border)
                                    };

                                    Frame::new()
                                        .fill(fill)
                                        .stroke(stroke)
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(Margin::symmetric(8, 2))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                let label_color = if is_active {
                                                    theme.foreground
                                                } else {
                                                    theme.foreground_muted
                                                };
                                                if ui
                                                    .add(
                                                        Button::new(
                                                            RichText::new(title)
                                                                .strong()
                                                                .color(label_color),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    switch_to = Some(idx);
                                                }

                                                if app.tabs.len() > 1
                                                    && ui
                                                        .add(
                                                            Button::new(
                                                                RichText::new("x")
                                                                    .color(theme.foreground_muted),
                                                            )
                                                            .fill(theme.surface_highest)
                                                            .corner_radius(CornerRadius::same(5))
                                                            .min_size(Vec2::new(22.0, 22.0)),
                                                        )
                                                        .on_hover_text("Close tab (Cmd+W)")
                                                        .clicked()
                                                {
                                                    close_tab_idx = Some(idx);
                                                }
                                            });
                                        });
                                }

                                if ui
                                    .add(
                                        Button::new(
                                            RichText::new("+ Tab").strong().color(theme.accent),
                                        )
                                        .fill(theme.surface_elevated)
                                        .stroke(Stroke::new(1.0, theme.border))
                                        .corner_radius(CornerRadius::same(8)),
                                    )
                                    .on_hover_text("New Tab (Cmd+T)")
                                    .clicked()
                                {
                                    app.new_tab();
                                }

                                if let Some(idx) = switch_to {
                                    app.active_tab = idx;
                                    app.focus_active_pane_editor();
                                }
                                if let Some(idx) = close_tab_idx {
                                    app.close_tab_at(idx);
                                }
                            });
                        });
                },
            );
        });
    });

    // Draw clean bottom hairline separator across the top bar (no edge-clipping box stroke).
    let rect = bar_resp.response.rect;
    ui.painter().line_segment(
        [
            Pos2::new(rect.left(), rect.bottom()),
            Pos2::new(rect.right(), rect.bottom()),
        ],
        Stroke::new(1.0, theme.border),
    );
}

/// Non-intrusive notification bar for config hot-reload errors or confirmation toasts.
pub fn render_notifications(ui: &mut Ui, app: &mut MaterialTerminalApp, theme: &MaterialTheme) {
    if let Some(err) = app.config_error.clone() {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                theme.error.r(),
                theme.error.g(),
                theme.error.b(),
                34,
            ))
            .stroke(Stroke::new(1.0, theme.error))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Config Error (previous valid config active): {err}"
                        ))
                        .color(theme.error),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            app.config_error = None;
                        }
                    });
                });
            });
    }

    if let Some((msg, expires_at)) = &app.toast {
        if std::time::Instant::now() >= *expires_at {
            app.toast = None;
        } else {
            let msg_text = msg.clone();
            Frame::new()
                .fill(theme.surface_highest)
                .stroke(Stroke::new(1.0, theme.border_focused))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(msg_text).color(theme.foreground));
                    });
                });
        }
    }
}

/// Inline search bar (`Cmd+F`) for finding text across CommandBlocks.
pub fn render_search_bar(
    ui: &mut Ui,
    app: &mut MaterialTerminalApp,
    theme: &MaterialTheme,
    focused_pane_id: u64,
) {
    if !app.search.open {
        return;
    }

    let match_ids: Vec<u64> = app
        .panes
        .get(&focused_pane_id)
        .map(|p| matching_block_ids(p.engine.blocks(), &app.search.query))
        .unwrap_or_default();

    if !match_ids.is_empty() {
        app.search.active_match_idx = app.search.active_match_idx.min(match_ids.len() - 1);
    } else {
        app.search.active_match_idx = 0;
    }

    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        app.search.open = false;
        app.focus_active_pane_editor();
        return;
    }

    Frame::new()
        .fill(theme.surface_elevated)
        .stroke(Stroke::new(1.0, theme.border_focused))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find:").strong().color(theme.accent));
                let search_id = Id::NULL.with("workspace_search_input");
                let resp = ui.add(
                    TextEdit::singleline(&mut app.search.query)
                        .id(search_id)
                        .desired_width(260.0)
                        .hint_text("Search commands & output..."),
                );
                if app.search.just_opened {
                    resp.request_focus();
                    app.search.just_opened = false;
                }
                if resp.changed() {
                    app.search.active_match_idx = 0;
                    app.search.scroll_to_active = true;
                }

                let count_label = if app.search.query.trim().is_empty() {
                    "0 matches".to_string()
                } else if match_ids.is_empty() {
                    "No matches".to_string()
                } else {
                    format!("{} / {}", app.search.active_match_idx + 1, match_ids.len())
                };
                ui.label(RichText::new(count_label).color(theme.foreground_muted));

                let enter_next = resp.has_focus()
                    && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                if !match_ids.is_empty()
                    && (ui.button("Prev").clicked()
                        || (resp.has_focus()
                            && ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter))))
                {
                    app.search.active_match_idx = if app.search.active_match_idx == 0 {
                        match_ids.len() - 1
                    } else {
                        app.search.active_match_idx - 1
                    };
                    app.search.scroll_to_active = true;
                }
                if !match_ids.is_empty() && (ui.button("Next").clicked() || enter_next) {
                    app.search.active_match_idx =
                        (app.search.active_match_idx + 1) % match_ids.len();
                    app.search.scroll_to_active = true;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        app.search.open = false;
                        app.focus_active_pane_editor();
                    }
                });
            });
        });
}

fn matching_block_ids(blocks: &[CommandBlock], query: &str) -> Vec<u64> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    blocks
        .iter()
        .filter(|b| {
            b.command_text.to_ascii_lowercase().contains(&q)
                || b.plain_output_text().to_ascii_lowercase().contains(&q)
        })
        .map(|b| b.id)
        .collect()
}

/// Recursively renders the split-pane binary tree with strictly disjoint clipped rects.
pub fn render_pane_tree(
    ui: &mut Ui,
    node: &mut PaneNode,
    rect: Rect,
    app: &mut MaterialTerminalApp,
    theme: &MaterialTheme,
    focused_pane_id: u64,
    active_search_block: Option<u64>,
) {
    if rect.width() < 20.0 || rect.height() < 20.0 {
        return;
    }

    match node {
        PaneNode::Leaf(pane_id) => {
            let pid = *pane_id;
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt(("pane_leaf", pid))
                    .max_rect(rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_clip_rect(rect);
                    render_single_pane(
                        ui,
                        pid,
                        rect,
                        app,
                        theme,
                        pid == focused_pane_id,
                        active_search_block,
                    );
                },
            );
        }
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let gap = 8.0;
            let (first_rect, handle_rect, second_rect) =
                compute_split_rects(rect, *axis, *ratio, gap);

            render_pane_tree(
                ui,
                first,
                first_rect,
                app,
                theme,
                focused_pane_id,
                active_search_block,
            );

            // Draggable Material divider
            let handle_id = ui.make_persistent_id((
                "split_handle",
                first_rect.min.x as i32,
                first_rect.min.y as i32,
                match axis {
                    SplitAxis::Vertical => 1u8,
                    SplitAxis::Horizontal => 2u8,
                },
            ));
            let resp = ui.interact(handle_rect, handle_id, Sense::drag());
            if resp.hovered() || resp.dragged() {
                let cursor = match axis {
                    SplitAxis::Vertical => CursorIcon::ResizeHorizontal,
                    SplitAxis::Horizontal => CursorIcon::ResizeVertical,
                };
                ui.ctx().set_cursor_icon(cursor);
            }
            if resp.dragged() {
                let delta = resp.drag_delta();
                match axis {
                    SplitAxis::Vertical => {
                        let usable_w = (rect.width() - gap).max(1.0);
                        *ratio = (*ratio + delta.x / usable_w).clamp(0.18, 0.82);
                    }
                    SplitAxis::Horizontal => {
                        let usable_h = (rect.height() - gap).max(1.0);
                        *ratio = (*ratio + delta.y / usable_h).clamp(0.18, 0.82);
                    }
                }
            }

            let divider_color = if resp.hovered() || resp.dragged() {
                theme.border_focused
            } else {
                theme.border
            };
            ui.painter()
                .rect_filled(handle_rect, CornerRadius::ZERO, theme.surface);
            let line_rect = match axis {
                SplitAxis::Vertical => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new(2.0, (handle_rect.height() - 12.0).max(8.0)),
                ),
                SplitAxis::Horizontal => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new((handle_rect.width() - 12.0).max(8.0), 2.0),
                ),
            };
            ui.painter()
                .rect_filled(line_rect, CornerRadius::same(1), divider_color);

            render_pane_tree(
                ui,
                second,
                second_rect,
                app,
                theme,
                focused_pane_id,
                active_search_block,
            );
        }
    }
}

pub fn compute_split_rects(
    rect: Rect,
    axis: SplitAxis,
    ratio: f32,
    gap: f32,
) -> (Rect, Rect, Rect) {
    match axis {
        SplitAxis::Vertical => {
            let usable_w = (rect.width() - gap).max(40.0);
            let min_side = (usable_w * 0.18).min(120.0);
            let first_w = (usable_w * ratio).clamp(min_side, usable_w - min_side);
            let r1 = Rect::from_min_max(rect.min, Pos2::new(rect.left() + first_w, rect.bottom()));
            let handle = Rect::from_min_max(
                Pos2::new(r1.right(), rect.top()),
                Pos2::new(r1.right() + gap, rect.bottom()),
            );
            let r2 = Rect::from_min_max(Pos2::new(handle.right(), rect.top()), rect.max);
            (r1, handle, r2)
        }
        SplitAxis::Horizontal => {
            let usable_h = (rect.height() - gap).max(40.0);
            let min_side = (usable_h * 0.18).min(100.0);
            let first_h = (usable_h * ratio).clamp(min_side, usable_h - min_side);
            let r1 = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + first_h));
            let handle = Rect::from_min_max(
                Pos2::new(rect.left(), r1.bottom()),
                Pos2::new(rect.right(), r1.bottom() + gap),
            );
            let r2 = Rect::from_min_max(Pos2::new(rect.left(), handle.bottom()), rect.max);
            (r1, handle, r2)
        }
    }
}

fn render_single_pane(
    ui: &mut Ui,
    pane_id: u64,
    rect: Rect,
    app: &mut MaterialTerminalApp,
    theme: &MaterialTheme,
    is_focused: bool,
    active_search_block: Option<u64>,
) {
    // Clicking anywhere inside an unfocused pane focuses it.
    let pointer_pressed_inside = ui.input(|i| {
        i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
    });
    if pointer_pressed_inside && !is_focused {
        app.set_focused_pane(pane_id);
    }

    let pane_count = app.active_pane_count();
    let cfg = app.config.clone();
    let pad_x = cfg.window.padding_x;
    let pad_y = cfg.window.padding_y;
    let font_size = cfg.font.size;
    let line_height = cfg.font.line_height;

    // Paint pane background and subtle inset focus border when multiple panes are open.
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, theme.background);
    if pane_count > 1 {
        let border_rect = rect.shrink(1.0);
        let stroke = if is_focused {
            Stroke::new(1.5, theme.border_focused)
        } else {
            Stroke::new(1.0, theme.border)
        };
        ui.painter().rect_stroke(
            border_rect,
            CornerRadius::same(6),
            stroke,
            egui::StrokeKind::Inside,
        );
    }

    let mut inner = rect.shrink2(Vec2::new(pad_x, pad_y));
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return;
    }

    // Optional compact pane header when split so user can identify & close any pane cleanly.
    let mut close_this_pane = false;
    if pane_count > 1 {
        let header_h = 34.0;
        let header_rect = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.right(), (inner.top() + header_h).min(inner.bottom())),
        );
        inner = Rect::from_min_max(
            Pos2::new(
                inner.left(),
                (header_rect.bottom() + 6.0).min(inner.bottom()),
            ),
            inner.max,
        );

        let pane_dir_label = app
            .panes
            .get(&pane_id)
            .map(|p| compact_home_path(p.engine.current_dir()))
            .unwrap_or_default();

        ui.scope_builder(
            UiBuilder::new()
                .id_salt(("pane_header", pane_id))
                .max_rect(header_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.set_clip_rect(header_rect);
                Frame::new()
                    .fill(theme.surface)
                    .stroke(Stroke::new(
                        1.0,
                        if is_focused {
                            theme.border_focused
                        } else {
                            theme.border
                        },
                    ))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let badge = if is_focused { "ACTIVE" } else { "PANE" };
                            let badge_color = if is_focused {
                                theme.accent
                            } else {
                                theme.foreground_subtle
                            };
                            ui.label(RichText::new(badge).small().strong().color(badge_color));
                            ui.label(
                                RichText::new(pane_dir_label)
                                    .small()
                                    .color(theme.foreground_muted),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        Button::new(
                                            RichText::new("Close").small().color(theme.error),
                                        )
                                        .fill(theme.surface_highest)
                                        .corner_radius(CornerRadius::same(5)),
                                    )
                                    .clicked()
                                {
                                    close_this_pane = true;
                                }
                            });
                        });
                    });
            },
        );
    }

    if close_this_pane {
        app.set_focused_pane(pane_id);
        app.close_active_pane(ui.ctx());
        return;
    }

    // Measure monospace cell geometry for accurate PTY sizing & Alt Screen rendering.
    let mono_font = FontId::monospace(font_size);
    let char_w = ui
        .fonts_mut(|f| f.glyph_width(&mono_font, 'W'))
        .max(font_size * 0.58);
    let cell_h = (font_size * line_height).ceil().max(font_size + 2.0);
    let term_cols = (inner.width() / char_w).floor().max(10.0) as usize;
    let term_rows = (inner.height() / cell_h).floor().max(4.0) as usize;

    let recent_history: Vec<String> = app
        .recent_history
        .iter()
        .map(|h| h.command.clone())
        .collect();
    let shortcuts_blocked = app.palette.open || app.search.open;
    let search_query = if app.search.open {
        app.search.query.clone()
    } else {
        String::new()
    };

    let mut deferred_block_action: Option<BlockAction> = None;
    let mut deferred_input_action: Option<InputAction> = None;
    let mut deferred_alt_bytes: Vec<u8> = Vec::new();
    let mut scroll_search_done = false;

    let (is_alt_screen, input_desired_h) = {
        let Some(pane) = app.panes.get_mut(&pane_id) else {
            return;
        };
        pane.pty.resize(term_cols as u16, term_rows as u16);
        pane.engine.resize(term_cols, term_rows);
        (
            pane.engine.is_alt_screen(),
            pane.editor.desired_height(ui, font_size, inner.width()),
        )
    };

    if is_alt_screen {
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(("pane_alt_screen", pane_id))
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(inner);
                if let Some(pane) = app.panes.get(&pane_id) {
                    deferred_alt_bytes = render_alt_screen(
                        ui,
                        pane,
                        inner,
                        theme,
                        font_size,
                        char_w,
                        cell_h,
                        is_focused && !shortcuts_blocked,
                    );
                }
            },
        );
    } else {
        // Compute strictly disjoint rects: [timeline_rect (top)] + [gap] + [input_rect (bottom)]
        let gap = 8.0;
        let input_h = input_desired_h.min((inner.height() * 0.48).max(76.0));
        let timeline_bottom = (inner.bottom() - input_h - gap).max(inner.top() + 30.0);
        let timeline_rect =
            Rect::from_min_max(inner.min, Pos2::new(inner.right(), timeline_bottom));
        let input_rect = Rect::from_min_max(
            Pos2::new(inner.left(), (timeline_bottom + gap).min(inner.bottom())),
            inner.max,
        );

        // 1. Scrollable CommandBlock timeline inside timeline_rect
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(("pane_timeline", pane_id))
                .max_rect(timeline_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(timeline_rect);
                let Some(pane) = app.panes.get_mut(&pane_id) else {
                    return;
                };

                if let Some(exit_code) = pane.exited {
                    Frame::new()
                        .fill(theme.surface_elevated)
                        .stroke(Stroke::new(1.0, theme.warning))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(format!(
                                    "Shell session exited (status {exit_code}). Press Cmd+W to close pane."
                                ))
                                .color(theme.warning),
                            );
                        });
                    ui.add_space(8.0);
                }

                let scroll_id = Id::NULL.with(("block_scroll", pane_id));
                ScrollArea::vertical()
                    .id_salt(scroll_id)
                    .max_height(ui.available_height())
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        if pane.engine.blocks().is_empty() {
                            render_welcome_empty_state(ui, pane, theme);
                        } else {
                            for block in pane.engine.blocks() {
                                let is_search_target = active_search_block == Some(block.id);
                                let act = render_block_card(
                                    ui,
                                    block,
                                    theme,
                                    font_size,
                                    line_height,
                                    &search_query,
                                    is_search_target,
                                );
                                if act != BlockAction::None {
                                    deferred_block_action = Some(act);
                                }
                                if is_search_target && app.search.scroll_to_active {
                                    ui.scroll_to_cursor(Some(Align::Center));
                                    scroll_search_done = true;
                                }
                                ui.add_space(cfg.window.block_spacing);
                            }
                        }

                        if pane.scroll_to_bottom {
                            let sentinel = ui.allocate_rect(
                                Rect::from_min_size(ui.cursor().min, Vec2::new(1.0, 1.0)),
                                Sense::hover(),
                            );
                            sentinel.scroll_to_me(Some(Align::BOTTOM));
                            pane.scroll_to_bottom = false;
                        }
                    });
            },
        );

        // 2. Anchored Input Editor inside input_rect (guaranteed visible, never covered)
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(("pane_input", pane_id))
                .max_rect(input_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(input_rect);
                let Some(pane) = app.panes.get_mut(&pane_id) else {
                    return;
                };
                let is_running = pane.engine.has_running_block();
                let input_action = pane.editor.show(
                    ui,
                    pane_id,
                    theme,
                    font_size,
                    pane.engine.current_dir(),
                    pane.engine.current_git_branch(),
                    is_running,
                    app.history.as_ref(),
                    &recent_history,
                    shortcuts_blocked || !is_focused,
                );
                if input_action != InputAction::None {
                    deferred_input_action = Some(input_action);
                }
            },
        );
    }

    if scroll_search_done {
        app.search.scroll_to_active = false;
    }

    if !deferred_alt_bytes.is_empty()
        && let Some(pane) = app.panes.get_mut(&pane_id)
    {
        pane.pty.write_bytes(&deferred_alt_bytes);
    }

    if let Some(act) = deferred_block_action {
        app.handle_block_action(ui.ctx(), pane_id, act);
    }
    if let Some(inp) = deferred_input_action {
        app.handle_input_action(pane_id, inp);
    }
}

fn render_welcome_empty_state(ui: &mut Ui, pane: &PaneState, theme: &MaterialTheme) {
    Frame::new()
        .fill(theme.surface)
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(theme.corner_radius.round() as u8))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new("Material Terminal")
                    .heading()
                    .strong()
                    .color(theme.foreground),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Working directory: {}   |   Shell: {}",
                    compact_home_path(pane.engine.current_dir()),
                    pane.pty.shell_program()
                ))
                .color(theme.foreground_muted),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Type a command below and press Enter.  Shortcuts: Cmd+K Commands  |  Cmd+D Split |  |  Cmd+Shift+D Split -  |  Cmd+T New Tab  |  Cmd+F Find",
                )
                .color(theme.foreground_subtle),
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn render_alt_screen(
    ui: &mut Ui,
    pane: &PaneState,
    rect: Rect,
    theme: &MaterialTheme,
    font_size: f32,
    char_w: f32,
    cell_h: f32,
    accepts_keyboard: bool,
) -> Vec<u8> {
    let _resp = ui.allocate_rect(rect, Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, theme.background);

    let grid = pane.engine.alt_screen();
    let (cursor_row, cursor_col) = grid.cursor();
    let origin = rect.min;

    for r in 0..grid.rows() {
        let row = grid.row_cells(r);
        let y = origin.y + (r as f32) * cell_h;
        if y > rect.bottom() {
            break;
        }

        // Paint non-default cell backgrounds in coalesced horizontal runs.
        let mut c = 0;
        while c < row.len() {
            let cell = row[c];
            let has_bg = cell.style.bg != TermColor::Default || cell.style.inverse;
            if !has_bg {
                c += 1;
                continue;
            }
            let bg_rgb = if cell.style.inverse {
                cell.style.fg.resolve_fg(&theme.palette)
            } else {
                cell.style
                    .bg
                    .resolve_bg(&theme.palette, theme.palette.background)
            };
            let bg_c32 = rgb_opaque(bg_rgb);
            let start_c = c;
            while c < row.len() {
                let next = row[c];
                let next_has_bg = next.style.bg != TermColor::Default || next.style.inverse;
                if !next_has_bg {
                    break;
                }
                let next_bg = if next.style.inverse {
                    next.style.fg.resolve_fg(&theme.palette)
                } else {
                    next.style
                        .bg
                        .resolve_bg(&theme.palette, theme.palette.background)
                };
                if rgb_opaque(next_bg) != bg_c32 {
                    break;
                }
                c += 1;
            }
            let bg_rect = Rect::from_min_size(
                Pos2::new(origin.x + (start_c as f32) * char_w, y),
                Vec2::new(((c - start_c) as f32) * char_w, cell_h),
            );
            painter.rect_filled(bg_rect, CornerRadius::ZERO, bg_c32);
        }

        // Coalesce foreground runs into one LayoutJob per row.
        let mut job = LayoutJob::default();
        let mut run_style = row[0].style;
        let mut run_text = String::with_capacity(row.len());
        for cell in row {
            if cell.style != run_style {
                if !run_text.is_empty() {
                    let mut fmt = theme.text_format_for(run_style, font_size);
                    fmt.background = Color32::TRANSPARENT;
                    job.append(&run_text, 0.0, fmt);
                    run_text.clear();
                }
                run_style = cell.style;
            }
            run_text.push(cell.ch);
        }
        if !run_text.is_empty() {
            let mut fmt = theme.text_format_for(run_style, font_size);
            fmt.background = Color32::TRANSPARENT;
            job.append(&run_text, 0.0, fmt);
        }
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(Pos2::new(origin.x, y), galley, theme.foreground);
    }

    // Paint terminal cursor
    if grid.cursor_visible && cursor_row < grid.rows() && cursor_col < grid.cols() {
        let cursor_rect = Rect::from_min_size(
            Pos2::new(
                origin.x + (cursor_col as f32) * char_w,
                origin.y + (cursor_row as f32) * cell_h,
            ),
            Vec2::new(char_w, cell_h),
        );
        painter.rect_filled(
            cursor_rect,
            CornerRadius::same(1),
            Color32::from_rgba_unmultiplied(
                theme.accent.r(),
                theme.accent.g(),
                theme.accent.b(),
                150,
            ),
        );
    }

    // Forward raw VT keystrokes / text / paste directly to PTY in Alt Screen mode.
    let mut out_bytes = Vec::new();
    if accepts_keyboard {
        let app_cursor = grid.application_cursor_keys;
        let bracketed = grid.bracketed_paste;
        ui.input(|input| {
            for ev in &input.events {
                match ev {
                    Event::Text(text) => {
                        if !input.modifiers.command && !input.modifiers.ctrl {
                            out_bytes.extend_from_slice(text.as_bytes());
                        }
                    }
                    Event::Paste(text) => {
                        if bracketed {
                            out_bytes.extend_from_slice(b"\x1b[200~");
                            out_bytes.extend_from_slice(text.as_bytes());
                            out_bytes.extend_from_slice(b"\x1b[201~");
                        } else {
                            out_bytes.extend_from_slice(text.as_bytes());
                        }
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        // Do not intercept global app shortcuts (`Cmd+...` on macOS)
                        if modifiers.mac_cmd
                            || (cfg!(not(target_os = "macos"))
                                && modifiers.command
                                && modifiers.shift)
                        {
                            continue;
                        }
                        if let Some(seq) = encode_egui_key(*key, *modifiers, app_cursor) {
                            out_bytes.extend_from_slice(&seq);
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    out_bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::ui::input_editor::InputEditorState;
    use egui::{Context, RawInput};

    #[test]
    fn split_rects_are_strictly_disjoint_for_both_axes() {
        let root = Rect::from_min_size(Pos2::new(0.0, 42.0), Vec2::new(1160.0, 698.0));

        let (v1, v_handle, v2) = compute_split_rects(root, SplitAxis::Vertical, 0.5, 8.0);
        assert!(v1.right() <= v_handle.left());
        assert!(v_handle.right() <= v2.left());
        assert!(!v1.intersects(v2));

        let (h1, h_handle, h2) = compute_split_rects(root, SplitAxis::Horizontal, 0.5, 8.0);
        assert!(h1.bottom() <= h_handle.top());
        assert!(h_handle.bottom() <= h2.top());
        assert!(!h1.intersects(h2));
    }

    #[test]
    fn single_pane_layout_keeps_typed_text_visible_at_bottom_without_overlap() {
        let ctx = Context::default();
        let cfg = AppConfig::default();
        let theme = MaterialTheme::from_config(&cfg);
        theme.apply_to_egui(&ctx, &cfg);

        let mut editor = InputEditorState::default();
        editor.set_text_and_focus("echo hello world".to_string());

        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 740.0))),
            ..Default::default()
        };

        let mut captured_timeline_rect = Rect::NOTHING;
        let mut captured_input_rect = Rect::NOTHING;

        let mut out = ctx.run_ui(raw.take(), |ui| {
            let inner = Rect::from_min_size(Pos2::new(14.0, 52.0), Vec2::new(1132.0, 678.0));
            let input_h = editor
                .desired_height(ui, cfg.font.size, inner.width())
                .min((inner.height() * 0.48).max(76.0));
            let gap = 8.0;
            let timeline_bottom = (inner.bottom() - input_h - gap).max(inner.top() + 30.0);
            captured_timeline_rect =
                Rect::from_min_max(inner.min, Pos2::new(inner.right(), timeline_bottom));
            captured_input_rect =
                Rect::from_min_max(Pos2::new(inner.left(), timeline_bottom + gap), inner.max);

            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("test_timeline")
                    .max_rect(captured_timeline_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_clip_rect(captured_timeline_rect);
                    ScrollArea::vertical()
                        .max_height(ui.available_height())
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.label("TIMELINE_CONTENT");
                        });
                },
            );

            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("test_input")
                    .max_rect(captured_input_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_clip_rect(captured_input_rect);
                    editor.show(
                        ui,
                        1,
                        &theme,
                        cfg.font.size,
                        std::path::Path::new("/tmp"),
                        Some("main"),
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });

        // Clear texture deltas cleanly before drop
        out.textures_delta.clear();

        assert!(!captured_timeline_rect.intersects(captured_input_rect));
        assert!(captured_input_rect.height() >= 70.0 && captured_input_rect.height() <= 100.0);
        assert!(captured_timeline_rect.height() > 500.0);

        let typed_shape = out.shapes.iter().find_map(|clipped| {
            if let egui::epaint::Shape::Text(t) = &clipped.shape
                && t.galley.text() == "echo hello world"
            {
                return Some((t.pos, clipped.clip_rect));
            }
            None
        });

        let (text_pos, clip) = typed_shape.expect("typed text galley must be painted");
        assert!(
            captured_input_rect.contains(text_pos),
            "typed text pos {text_pos:?} must reside inside input_rect {captured_input_rect:?}"
        );
        assert!(
            clip.intersects(captured_input_rect) && clip.height() > 10.0,
            "typed text clip rect {clip:?} must be visible inside input_rect"
        );
    }

    #[test]
    fn typing_events_are_accepted_and_rendered_inside_editor() {
        let ctx = Context::default();
        let cfg = AppConfig::default();
        let theme = MaterialTheme::from_config(&cfg);
        theme.apply_to_egui(&ctx, &cfg);

        let mut editor = InputEditorState::default();
        let input_rect = Rect::from_min_size(Pos2::new(14.0, 620.0), Vec2::new(1132.0, 90.0));

        // Frame 1: focus the editor
        let raw1 = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 740.0))),
            ..Default::default()
        };
        let mut out1 = ctx.run_ui(raw1, |ui| {
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("typing_test_1")
                    .max_rect(input_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    editor.show(
                        ui,
                        99,
                        &theme,
                        cfg.font.size,
                        std::path::Path::new("/tmp"),
                        None,
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });
        out1.textures_delta.clear();

        // Frame 2: send real Text event "ls -la"
        let raw2 = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 740.0))),
            events: vec![Event::Text("ls -la".to_string())],
            ..Default::default()
        };
        let mut out2 = ctx.run_ui(raw2, |ui| {
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("typing_test_1")
                    .max_rect(input_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    editor.show(
                        ui,
                        99,
                        &theme,
                        cfg.font.size,
                        std::path::Path::new("/tmp"),
                        None,
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });
        out2.textures_delta.clear();

        assert_eq!(editor.buffer, "ls -la");
        let raw3 = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 740.0))),
            ..Default::default()
        };
        let mut out3 = ctx.run_ui(raw3, |ui| {
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("typing_test_1")
                    .max_rect(input_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    editor.show(
                        ui,
                        99,
                        &theme,
                        cfg.font.size,
                        std::path::Path::new("/tmp"),
                        None,
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });
        out3.textures_delta.clear();
        let found = out3.shapes.iter().any(
            |c| matches!(&c.shape, egui::epaint::Shape::Text(t) if t.galley.text() == "ls -la"),
        );
        assert!(found, "typed text 'ls -la' must appear in painted frame");
    }
}
