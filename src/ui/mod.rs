//! Workspace UI layout: minimalist top tab bar, recursive draggable split panes,
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
    Align, Color32, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Layout, Margin,
    Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};

/// Minimalist top tab strip with active theme chip and Command Palette / Search buttons.
pub fn render_top_bar(ui: &mut Ui, app: &mut MaterialTerminalApp, theme: &MaterialTheme) {
    let bar_frame = Frame::new()
        .fill(theme.surface)
        .stroke(Stroke::new(1.0, theme.border))
        .inner_margin(Margin::symmetric(10, 6));

    bar_frame.show(ui, |ui| {
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
                    Color32::TRANSPARENT
                };
                let stroke = if is_active {
                    Stroke::new(1.0, theme.border_focused)
                } else {
                    Stroke::new(1.0, Color32::TRANSPARENT)
                };

                let pill = Frame::new()
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 4));

                let tab_id = Id::NULL.with(("tab_pill", tab.id));
                let resp = pill
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let label_color = if is_active {
                                theme.foreground
                            } else {
                                theme.foreground_muted
                            };
                            let lbl = ui.add(
                                egui::Label::new(
                                    RichText::new(title).small().strong().color(label_color),
                                )
                                .sense(Sense::click()),
                            );
                            if lbl.clicked() {
                                switch_to = Some(idx);
                            }

                            if app.tabs.len() > 1
                                && ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("×")
                                                .small()
                                                .color(theme.foreground_subtle),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Close tab (Cmd+W)")
                                    .clicked()
                            {
                                close_tab_idx = Some(idx);
                            }
                        });
                    })
                    .response
                    .interact(Sense::click());

                if resp.clicked() && switch_to.is_none() {
                    switch_to = Some(idx);
                }
                let _ = tab_id;
            }

            if ui
                .small_button(RichText::new("+").strong().color(theme.accent))
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

            // Right side: Split quick actions, Search, Command Palette, Active Theme pill
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let theme_label = app.config.resolved_theme().display_name();
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("Theme: {theme_label}"))
                                .small()
                                .color(theme.accent),
                        )
                        .fill(theme.accent_soft)
                        .corner_radius(CornerRadius::same(7)),
                    )
                    .on_hover_text("Switch theme or open Command Palette (Cmd+K)")
                    .clicked()
                {
                    app.palette.toggle();
                }

                if ui
                    .small_button(RichText::new("Cmd+K").small().color(theme.foreground_muted))
                    .on_hover_text("Open Command Palette (Cmd+K / Ctrl+Shift+P)")
                    .clicked()
                {
                    app.palette.toggle();
                }

                if ui
                    .small_button(RichText::new("Find").small().color(theme.foreground_muted))
                    .on_hover_text("Search blocks (Cmd+F)")
                    .clicked()
                {
                    app.toggle_search();
                }

                if ui
                    .small_button(
                        RichText::new("Split ⊟")
                            .small()
                            .color(theme.foreground_muted),
                    )
                    .on_hover_text("Split Horizontally (Cmd+Shift+D)")
                    .clicked()
                {
                    app.split_focused_pane(SplitAxis::Horizontal);
                }

                if ui
                    .small_button(
                        RichText::new("Split ⊞")
                            .small()
                            .color(theme.foreground_muted),
                    )
                    .on_hover_text("Split Vertically (Cmd+D)")
                    .clicked()
                {
                    app.split_focused_pane(SplitAxis::Vertical);
                }
            });
        });
    });
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
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Config Error (previous valid config active): {err}"
                        ))
                        .small()
                        .color(theme.error),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Dismiss").clicked() {
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
                .inner_margin(Margin::symmetric(12, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(msg_text).small().color(theme.foreground));
                    });
                });
        }
    }
}

/// Inline floating search bar (`Cmd+F`) for finding text across CommandBlocks.
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
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find:").small().color(theme.accent));
                let search_id = Id::NULL.with("workspace_search_input");
                let resp = ui.add(
                    TextEdit::singleline(&mut app.search.query)
                        .id(search_id)
                        .desired_width(240.0)
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
                ui.label(
                    RichText::new(count_label)
                        .small()
                        .color(theme.foreground_muted),
                );

                let enter_next = resp.has_focus()
                    && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                if !match_ids.is_empty()
                    && (ui.small_button("Prev").clicked()
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
                if !match_ids.is_empty() && (ui.small_button("Next").clicked() || enter_next) {
                    app.search.active_match_idx =
                        (app.search.active_match_idx + 1) % match_ids.len();
                    app.search.scroll_to_active = true;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Close").clicked() {
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

/// Recursively renders the split-pane binary tree inside the workspace.
pub fn render_pane_tree(
    ui: &mut Ui,
    node: &mut PaneNode,
    rect: Rect,
    app: &mut MaterialTerminalApp,
    theme: &MaterialTheme,
    focused_pane_id: u64,
    active_search_block: Option<u64>,
) {
    match node {
        PaneNode::Leaf(pane_id) => {
            let pid = *pane_id;
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            render_single_pane(
                &mut child_ui,
                pid,
                rect,
                app,
                theme,
                pid == focused_pane_id,
                active_search_block,
            );
        }
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let gap = 6.0;
            let (first_rect, handle_rect, second_rect) = match axis {
                SplitAxis::Vertical => {
                    let usable_w = (rect.width() - gap).max(40.0);
                    let first_w = (usable_w * *ratio).clamp(80.0, usable_w - 80.0);
                    let r1 = Rect::from_min_max(
                        rect.min,
                        Pos2::new(rect.left() + first_w, rect.bottom()),
                    );
                    let handle = Rect::from_min_max(
                        Pos2::new(r1.right(), rect.top()),
                        Pos2::new(r1.right() + gap, rect.bottom()),
                    );
                    let r2 = Rect::from_min_max(Pos2::new(handle.right(), rect.top()), rect.max);
                    (r1, handle, r2)
                }
                SplitAxis::Horizontal => {
                    let usable_h = (rect.height() - gap).max(40.0);
                    let first_h = (usable_h * *ratio).clamp(80.0, usable_h - 80.0);
                    let r1 =
                        Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + first_h));
                    let handle = Rect::from_min_max(
                        Pos2::new(rect.left(), r1.bottom()),
                        Pos2::new(rect.right(), r1.bottom() + gap),
                    );
                    let r2 = Rect::from_min_max(Pos2::new(rect.left(), handle.bottom()), rect.max);
                    (r1, handle, r2)
                }
            };

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
                        *ratio = (*ratio + delta.x / usable_w).clamp(0.15, 0.85);
                    }
                    SplitAxis::Horizontal => {
                        let usable_h = (rect.height() - gap).max(1.0);
                        *ratio = (*ratio + delta.y / usable_h).clamp(0.15, 0.85);
                    }
                }
            }
            let divider_color = if resp.hovered() || resp.dragged() {
                theme.border_focused
            } else {
                theme.border
            };
            let line_rect = match axis {
                SplitAxis::Vertical => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new(1.5, handle_rect.height() - 8.0),
                ),
                SplitAxis::Horizontal => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new(handle_rect.width() - 8.0, 1.5),
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

    let cfg = app.config.clone();
    let pad_x = cfg.window.padding_x;
    let pad_y = cfg.window.padding_y;
    let font_size = cfg.font.size;
    let line_height = cfg.font.line_height;

    // Measure monospace cell geometry for accurate PTY sizing & Alt Screen rendering.
    let mono_font = FontId::monospace(font_size);
    let char_w = ui
        .fonts_mut(|f| f.glyph_width(&mono_font, 'W'))
        .max(font_size * 0.58);
    let cell_h = (font_size * line_height).ceil().max(font_size + 2.0);
    let avail_w = (rect.width() - pad_x * 2.0).max(char_w * 10.0);
    let avail_h = (rect.height() - pad_y * 2.0).max(cell_h * 4.0);
    let term_cols = (avail_w / char_w).floor().max(10.0) as usize;
    let term_rows = (avail_h / cell_h).floor().max(4.0) as usize;

    let frame = Frame::new()
        .fill(theme.background)
        .stroke(Stroke::new(
            if is_focused && app.active_pane_count() > 1 {
                1.2
            } else {
                0.0
            },
            theme.border_focused,
        ))
        .inner_margin(Margin::symmetric(pad_x.round() as i8, pad_y.round() as i8));

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

    frame.show(ui, |ui| {
        ui.set_clip_rect(rect);

        let Some(pane) = app.panes.get_mut(&pane_id) else {
            return;
        };

        // Keep PTY and AltScreenGrid synchronized with pane pixel dimensions.
        pane.pty.resize(term_cols as u16, term_rows as u16);
        pane.engine.resize(term_cols, term_rows);

        if pane.engine.is_alt_screen() {
            deferred_alt_bytes = render_alt_screen(
                ui,
                pane,
                theme,
                font_size,
                char_w,
                cell_h,
                is_focused && !shortcuts_blocked,
            );
            return;
        }

        // Standard Block Mode: anchored Input Editor at bottom + scrollable CommandBlocks above.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
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

            ui.add_space(8.0);

            // Remaining space above input editor hosts the CommandBlock timeline.
            let scroll_avail = ui.available_size();
            ui.allocate_ui_with_layout(
                scroll_avail,
                Layout::top_down(Align::Min),
                |ui| {
                    if let Some(exit_code) = pane.exited {
                        Frame::new()
                            .fill(theme.surface_elevated)
                            .stroke(Stroke::new(1.0, theme.warning))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
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
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
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
        });
    });

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
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Ready in {} · Shell: {}",
                    compact_home_path(pane.engine.current_dir()),
                    pane.pty.shell_program()
                ))
                .color(theme.foreground_muted),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Cmd+K Command Palette  ·  Cmd+D Split Right  ·  Cmd+Shift+D Split Down  ·  Cmd+T New Tab  ·  Cmd+F Find",
                )
                .small()
                .color(theme.foreground_subtle),
            );
        });
}

fn render_alt_screen(
    ui: &mut Ui,
    pane: &PaneState,
    theme: &MaterialTheme,
    font_size: f32,
    char_w: f32,
    cell_h: f32,
    accepts_keyboard: bool,
) -> Vec<u8> {
    let avail = ui.available_size();
    let (rect, _resp) = ui.allocate_exact_size(avail, Sense::click());
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
