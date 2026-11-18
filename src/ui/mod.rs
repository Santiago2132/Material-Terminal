//! Liquid Glass workspace UI layout: integrated titlebar, clean glass tabs,
//! deterministic non-overlapping split panes, Block Mode scroll view, and Alt Screen GPU grid.

pub mod block_view;
pub mod command_palette;
pub mod input_editor;
pub mod theme;

use crate::app::{MaterialTerminalApp, PaneNode, PaneState, SplitAxis};
use crate::blocks::CommandBlock;
use crate::terminal::{TermColor, encode_egui_key};
use crate::ui::block_view::{BlockAction, render_block_card};
use crate::ui::input_editor::{InputAction, compact_home_path};
use crate::ui::theme::{MaterialTheme, paint_specular_highlight, rgb_opaque};
use egui::text::LayoutJob;
use egui::{
    Align, Button, Color32, CornerRadius, CursorIcon, Event, FontId, Frame, Id, Key, Layout,
    Margin, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, Vec2, ViewportCommand,
};

/// Integrated 46px Liquid Glass titlebar with macOS traffic-light margin, tabs, and toolbar pills.
pub fn render_top_bar(ui: &mut Ui, app: &mut MaterialTerminalApp, theme: &MaterialTheme) -> Rect {
    let full = ui.available_rect_before_wrap();
    let bar_rect = Rect::from_min_size(full.min, Vec2::new(full.width(), 46.0));

    ui.painter()
        .rect_filled(bar_rect, CornerRadius::ZERO, theme.glass_titlebar);

    let left_pad = if cfg!(target_os = "macos") {
        80.0
    } else {
        14.0
    };
    let content_rect = Rect::from_min_max(
        Pos2::new(bar_rect.left() + left_pad, bar_rect.top() + 6.0),
        Pos2::new(
            (bar_rect.right() - 14.0).max(bar_rect.left() + left_pad + 40.0),
            bar_rect.bottom() - 6.0,
        ),
    );

    let palette_label = if cfg!(target_os = "macos") {
        "Cmd+K"
    } else {
        "Ctrl+Shift+P"
    };

    // Tab pills / '+' button rects that must never start a native window drag.
    let mut no_drag_rects: Vec<Rect> = Vec::new();
    ui.scope_builder(
        UiBuilder::new()
            .id_salt("material_top_bar_scope")
            .max_rect(content_rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.set_clip_rect(bar_rect);
            ui.spacing_mut().item_spacing.x = 8.0;

            if ui
                .add(
                    Button::new(
                        RichText::new(palette_label)
                            .small()
                            .color(theme.foreground_muted),
                    )
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 13))
                    .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                    .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Open Command Palette (Cmd+K / Ctrl+Shift+P)")
                .clicked()
            {
                app.palette.toggle();
            }

            let theme_label = app.config.resolved_theme().display_name();
            if ui
                .add(
                    Button::new(
                        RichText::new(format!("Theme: {theme_label}"))
                            .small()
                            .strong()
                            .color(theme.accent),
                    )
                    .fill(theme.accent_soft)
                    .stroke(Stroke::new(
                        1.0,
                        Color32::from_rgba_unmultiplied(
                            theme.accent.r(),
                            theme.accent.g(),
                            theme.accent.b(),
                            90,
                        ),
                    ))
                    .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Click to cycle theme (or Cmd+K for all themes)")
                .clicked()
            {
                app.cycle_theme();
            }

            if ui
                .add(
                    Button::new(RichText::new("Find").small().color(theme.foreground_muted))
                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 13))
                        .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Search Blocks (Cmd+F)")
                .clicked()
            {
                app.toggle_search();
            }

            if ui
                .add(
                    Button::new(
                        RichText::new("Split -")
                            .small()
                            .color(theme.foreground_muted),
                    )
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 13))
                    .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                    .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Split Horizontally Top/Bottom")
                .clicked()
            {
                app.split_focused_pane(SplitAxis::Horizontal);
            }

            if ui
                .add(
                    Button::new(
                        RichText::new("Split |")
                            .small()
                            .color(theme.foreground_muted),
                    )
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 13))
                    .stroke(Stroke::new(1.0, theme.glass_border_subtle))
                    .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Split Vertically Side-by-Side")
                .clicked()
            {
                app.split_focused_pane(SplitAxis::Vertical);
            }

            // Remaining left space: the tab strip. It spans the FULL titlebar height so the
            // fixed-height rounded pills are vertically centered with clearance on both sides
            // and are never sliced flat where the terminal workspace begins.
            let tabs_avail_w = ui.available_width().max(80.0);
            let strip_rect = Rect::from_min_max(
                Pos2::new(content_rect.left(), bar_rect.top()),
                Pos2::new(
                    (content_rect.left() + tabs_avail_w).min(content_rect.right()),
                    bar_rect.bottom(),
                ),
            );
            let tab_infos: Vec<TabPillInfo> = app
                .tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| TabPillInfo {
                    title: app.tab_title(tab),
                    badge: app.tab_shell_badge(tab),
                    is_active: idx == app.active_tab,
                })
                .collect();
            let strip = render_tab_strip(ui, strip_rect, &tab_infos, theme);
            no_drag_rects = strip.interactive_rects.clone();
            if strip.new_tab {
                app.new_tab();
            } else if let Some(idx) = strip.close_idx {
                app.close_tab_at(idx);
            } else if let Some(idx) = strip.switch_to {
                app.active_tab = idx;
                app.focus_active_pane_editor();
            }
        },
    );

    // Enable native window dragging from empty titlebar space without stealing button clicks.
    if !ui.ctx().egui_is_using_pointer() {
        let drag_press = ui.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer.press_origin().is_some_and(|p| {
                    bar_rect.contains(p) && !no_drag_rects.iter().any(|r| r.contains(p))
                })
        });
        if drag_press {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        } else if ui.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| bar_rect.contains(p))
        }) {
            let currently_max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!currently_max));
        }
    }

    // Crisp bottom glass hairline separator
    ui.painter().line_segment(
        [
            Pos2::new(bar_rect.left(), bar_rect.bottom()),
            Pos2::new(bar_rect.right(), bar_rect.bottom()),
        ],
        Stroke::new(1.0, theme.glass_border_subtle),
    );
    ui.allocate_rect(bar_rect, Sense::hover());
    bar_rect
}

/// Fixed height of a rounded tab pill. Inside the 46px titlebar this leaves 9px of clearance
/// above and below, so the 10px rounded corners are never clipped by the workspace edge.
pub(crate) const TAB_PILL_HEIGHT: f32 = 28.0;
/// Corner radius of a tab pill (matches the mockup's `.tab { border-radius: 10px }`).
const TAB_PILL_RADIUS: u8 = 10;
/// Maximum number of characters shown in a tab title before ellipsizing.
const TAB_TITLE_MAX_CHARS: usize = 24;

/// Display data for one tab pill in the titlebar strip.
pub(crate) struct TabPillInfo {
    pub title: String,
    pub badge: String,
    pub is_active: bool,
}

/// User interactions and painted geometry produced by [`render_tab_strip`].
#[derive(Default)]
pub(crate) struct TabStripResponse {
    pub switch_to: Option<usize>,
    pub close_idx: Option<usize>,
    pub new_tab: bool,
    /// Full rect of every tab pill, in tab order.
    pub pill_rects: Vec<Rect>,
    /// Pills + '+' button: regions that must not trigger native window dragging.
    pub interactive_rects: Vec<Rect>,
}

fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Paints the titlebar tab strip with deterministic geometry.
///
/// Every pill is painted directly as a single rounded shape (fill + stroke) of fixed height,
/// vertically centered in `strip_rect`. No egui `Button`/`Frame` widgets are nested inside the
/// pill, so no rectangular hover/focus frame can ever be drawn over the rounded tab, and long
/// running-command titles (e.g. `▶ antigravity`) are ellipsized instead of overflowing.
/// Overflowing tabs scroll horizontally with the mouse wheel / trackpad.
pub(crate) fn render_tab_strip(
    ui: &mut Ui,
    strip_rect: Rect,
    tabs: &[TabPillInfo],
    theme: &MaterialTheme,
) -> TabStripResponse {
    let mut out = TabStripResponse::default();
    if strip_rect.width() < 8.0 || strip_rect.height() < 8.0 {
        return out;
    }

    let painter = ui.painter_at(strip_rect);
    let pill_h = TAB_PILL_HEIGHT.min(strip_rect.height() - 6.0);
    let pill_top = strip_rect.center().y - pill_h / 2.0;
    let can_close = tabs.len() > 1;

    let pad_x = 10.0;
    let inner_gap = 6.0;
    let pill_gap = 6.0;
    let close_size = 18.0;
    let plus_size = Vec2::new(28.0, 26.0);

    let title_font = FontId::proportional(12.5);
    let badge_font = FontId::monospace(10.5);

    // Pre-measure every pill so the strip can be laid out and scrolled deterministically.
    let measured: Vec<_> = tabs
        .iter()
        .map(|tab| {
            let title_color = if tab.is_active {
                theme.foreground
            } else {
                theme.foreground_muted
            };
            let title = painter.layout_no_wrap(
                ellipsize(&tab.title, TAB_TITLE_MAX_CHARS),
                title_font.clone(),
                title_color,
            );
            let badge = painter.layout_no_wrap(tab.badge.clone(), badge_font.clone(), theme.accent);
            let badge_w = badge.size().x + 10.0;
            let mut width = pad_x + title.size().x + inner_gap + badge_w + pad_x;
            if can_close {
                width += inner_gap + close_size - 4.0;
            }
            (title, badge, badge_w, width)
        })
        .collect();

    let content_w: f32 = measured.iter().map(|m| m.3 + pill_gap).sum::<f32>() + plus_size.x + 4.0;

    // Horizontal scroll offset (persisted per strip).
    let strip_id = Id::NULL.with("material_tab_strip");
    let max_offset = (content_w - strip_rect.width()).max(0.0);
    let mut offset: f32 = ui.data(|d| d.get_temp(strip_id)).unwrap_or(0.0);
    let strip_hovered = ui.rect_contains_pointer(strip_rect);
    if strip_hovered && max_offset > 0.0 {
        let delta = ui.input(|i| i.smooth_scroll_delta());
        offset -= delta.x + delta.y;
    }
    offset = offset.clamp(0.0, max_offset);
    ui.data_mut(|d| d.insert_temp(strip_id, offset));

    let mut x = strip_rect.left() + 2.0 - offset;
    for (idx, (tab, (title, badge, badge_w, width))) in tabs.iter().zip(measured).enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x, pill_top), Vec2::new(width, pill_h));
        x += width + pill_gap;
        out.pill_rects.push(rect);
        out.interactive_rects.push(rect);

        let pill_resp = ui
            .interact(rect, strip_id.with(("tab", idx)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let close_rect = can_close.then(|| {
            Rect::from_center_size(
                Pos2::new(
                    rect.right() - pad_x + 2.0 - close_size / 2.0,
                    rect.center().y,
                ),
                Vec2::splat(close_size),
            )
        });
        let close_resp = close_rect.map(|r| {
            ui.interact(r, strip_id.with(("tab_close", idx)), Sense::click())
                .on_hover_text("Close tab")
        });
        let close_hovered = close_resp.as_ref().is_some_and(|r| r.hovered());
        let hovered = pill_resp.hovered() || close_hovered;

        let fill = if tab.is_active {
            Color32::from_rgba_unmultiplied(255, 255, 255, 22)
        } else if hovered {
            Color32::from_rgba_unmultiplied(255, 255, 255, 10)
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if tab.is_active {
            Stroke::new(1.0, theme.glass_border)
        } else if hovered {
            Stroke::new(1.0, theme.glass_border_subtle)
        } else {
            Stroke::NONE
        };
        painter.rect(
            rect,
            CornerRadius::same(TAB_PILL_RADIUS),
            fill,
            stroke,
            StrokeKind::Inside,
        );
        if tab.is_active {
            paint_specular_highlight(
                &painter,
                rect,
                f32::from(TAB_PILL_RADIUS),
                Color32::from_rgba_unmultiplied(255, 255, 255, 36),
            );
        }

        // Title
        let title_pos = Pos2::new(rect.left() + pad_x, rect.center().y - title.size().y / 2.0);
        let title_w = title.size().x;
        painter.galley(title_pos, title, theme.foreground);

        // Shell badge
        let badge_rect = Rect::from_min_size(
            Pos2::new(
                title_pos.x + title_w + inner_gap,
                rect.center().y - (badge.size().y + 2.0) / 2.0,
            ),
            Vec2::new(badge_w, badge.size().y + 2.0),
        );
        painter.rect_filled(badge_rect, CornerRadius::same(5), theme.accent_soft);
        let badge_pos = badge_rect.center() - badge.size() / 2.0;
        painter.galley(badge_pos, badge, theme.accent);

        // Close button (rounded, only highlighted on hover)
        if let (Some(close_rect), Some(close_resp)) = (close_rect, close_resp.as_ref()) {
            if close_hovered {
                painter.rect_filled(
                    close_rect,
                    CornerRadius::same(5),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                );
            }
            painter.text(
                close_rect.center(),
                egui::Align2::CENTER_CENTER,
                "×",
                FontId::proportional(13.0),
                if close_hovered {
                    theme.error
                } else {
                    theme.foreground_muted
                },
            );
            if close_resp.clicked() {
                out.close_idx = Some(idx);
                continue;
            }
        }

        if pill_resp.clicked() {
            out.switch_to = Some(idx);
        }
    }

    // '+' new tab button
    let plus_rect = Rect::from_min_size(
        Pos2::new(x, strip_rect.center().y - plus_size.y / 2.0),
        plus_size,
    );
    out.interactive_rects.push(plus_rect);
    let plus_resp = ui
        .interact(plus_rect, strip_id.with("new_tab"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("New Tab");
    painter.rect(
        plus_rect,
        CornerRadius::same(8),
        if plus_resp.hovered() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 28)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 14)
        },
        Stroke::new(1.0, theme.glass_border_subtle),
        StrokeKind::Inside,
    );
    painter.text(
        plus_rect.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        FontId::proportional(15.0),
        theme.accent,
    );
    if plus_resp.clicked() {
        out.new_tab = true;
    }

    out
}

/// Non-intrusive notification bar for config hot-reload errors or confirmation toasts.
pub fn render_notifications(ui: &mut Ui, app: &mut MaterialTerminalApp, theme: &MaterialTheme) {
    if let Some(err) = app.config_error.clone() {
        Frame::new()
            .fill(Color32::from_rgba_unmultiplied(
                theme.error.r(),
                theme.error.g(),
                theme.error.b(),
                36,
            ))
            .stroke(Stroke::new(1.0, theme.error))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(14, 6))
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
                .fill(Color32::from_rgba_unmultiplied(
                    theme.surface_elevated.r(),
                    theme.surface_elevated.g(),
                    theme.surface_elevated.b(),
                    220,
                ))
                .stroke(Stroke::new(1.0, theme.glass_border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(14, 6))
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
        .fill(theme.glass_input)
        .stroke(Stroke::new(1.0, theme.border_focused))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 7))
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

            // Draggable glass splitter handle
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
                theme.accent
            } else {
                theme.glass_border_subtle
            };
            let line_rect = match axis {
                SplitAxis::Vertical => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new(2.0, (handle_rect.height() - 16.0).max(8.0)),
                ),
                SplitAxis::Horizontal => Rect::from_center_size(
                    handle_rect.center(),
                    Vec2::new((handle_rect.width() - 16.0).max(8.0), 2.0),
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
    // Clicking anywhere inside a pane focuses that pane and its input editor.
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

    if pane_count > 1 {
        let border_rect = rect.shrink(2.0);
        let stroke = if is_focused {
            Stroke::new(1.3, theme.accent)
        } else {
            Stroke::new(1.0, theme.glass_border_subtle)
        };
        ui.painter().rect_stroke(
            border_rect,
            CornerRadius::same(10),
            stroke,
            StrokeKind::Inside,
        );
    }

    let mut inner = rect.shrink2(Vec2::new(pad_x, pad_y));
    if inner.width() < 40.0 || inner.height() < 40.0 {
        return;
    }

    // Compact glass pane header only when split so each pane can be identified/closed cleanly.
    let mut close_this_pane = false;
    if pane_count > 1 {
        let header_h = 32.0;
        let header_rect = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.right(), (inner.top() + header_h).min(inner.bottom())),
        );
        inner = Rect::from_min_max(
            Pos2::new(
                inner.left(),
                (header_rect.bottom() + 8.0).min(inner.bottom()),
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
                    .fill(theme.glass_block)
                    .stroke(Stroke::new(
                        1.0,
                        if is_focused {
                            theme.accent
                        } else {
                            theme.glass_border_subtle
                        },
                    ))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 3))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let badge = if is_focused { "ACTIVE" } else { "PANE" };
                            let badge_color = if is_focused {
                                theme.accent
                            } else {
                                theme.foreground_subtle
                            };
                            ui.label(
                                RichText::new(badge)
                                    .small()
                                    .monospace()
                                    .strong()
                                    .color(badge_color),
                            );
                            ui.label(
                                RichText::new(pane_dir_label)
                                    .small()
                                    .monospace()
                                    .color(theme.foreground_muted),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        Button::new(
                                            RichText::new("Close").small().color(theme.error),
                                        )
                                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 14))
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
    // PTY geometry always matches the inset Alt Screen grid so entering/leaving a TUI never
    // triggers a resize and full-screen TUIs never paint over the rounded card corners.
    let grid_rect = alt_screen_grid_rect(inner);
    let term_cols = (grid_rect.width() / char_w).floor().max(10.0) as usize;
    let term_rows = (grid_rect.height() / cell_h).floor().max(4.0) as usize;

    let recent_history: Vec<String> = app
        .recent_history
        .iter()
        .map(|h| h.command.clone())
        .collect();
    let shell_name = app.pane_shell_name(pane_id);
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
        // Strictly disjoint vertical split: [timeline_rect] + [gap] + [input_rect]
        let gap = 10.0;
        let input_h = input_desired_h.min((inner.height() * 0.48).max(78.0));
        let timeline_bottom = (inner.bottom() - input_h - gap).max(inner.top() + 30.0);
        let timeline_rect =
            Rect::from_min_max(inner.min, Pos2::new(inner.right(), timeline_bottom));
        let input_rect = Rect::from_min_max(
            Pos2::new(inner.left(), (timeline_bottom + gap).min(inner.bottom())),
            inner.max,
        );

        // 1. Scrollable CommandBlocks inside timeline_rect
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
                        .fill(theme.glass_block)
                        .stroke(Stroke::new(1.0, theme.warning))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(format!(
                                    "Shell session exited (status {exit_code}). Press Cmd+W to close pane."
                                ))
                                .color(theme.warning),
                            );
                        });
                    ui.add_space(10.0);
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
                            render_welcome_empty_state(ui, pane, theme, font_size);
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

        // 2. Floating Glass Input Dock inside input_rect
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
                    &shell_name,
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

fn render_welcome_empty_state(
    ui: &mut Ui,
    pane: &PaneState,
    theme: &MaterialTheme,
    font_size: f32,
) {
    let radius = 14.0_f32;
    let card = Frame::new()
        .fill(theme.glass_block)
        .stroke(Stroke::new(1.0, theme.glass_border_subtle))
        .corner_radius(CornerRadius::same(radius as u8))
        .inner_margin(Margin {
            left: 18,
            right: 16,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(compact_home_path(pane.engine.current_dir()))
                        .monospace()
                        .strong()
                        .color(theme.accent),
                );
                if let Some(branch) = pane.engine.current_git_branch() {
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
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "> Material Terminal Ready ({}) — GPU Metal 120 FPS",
                    pane.pty.shell_program()
                ))
                .font(FontId::monospace(font_size))
                .strong()
                .color(Color32::WHITE),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Type any shell command below and press Enter.\nShortcuts: Cmd+K Palette  |  Cmd+D Split |  |  Cmd+Shift+D Split -  |  Cmd+T New Tab  |  Cmd+F Find",
                )
                .font(FontId::monospace((font_size - 0.8).max(11.0)))
                .color(theme.foreground_muted),
            );
        });

    let rect = card.response.rect;
    paint_specular_highlight(
        ui.painter(),
        rect,
        radius,
        Color32::from_rgba_unmultiplied(255, 255, 255, 18),
    );
    let strip_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 12.0),
        Pos2::new(rect.left() + 3.5, rect.bottom() - 12.0),
    );
    ui.painter()
        .rect_filled(strip_rect, CornerRadius::same(2), theme.accent);
}

/// Corner radius of the Alt Screen card (matches the mockup's 14px block cards).
pub(crate) const ALT_SCREEN_RADIUS: u8 = 14;
/// Inset between the Alt Screen card edge and the terminal cell grid. Large enough that the
/// grid's corners lie inside the rounded arc of the card.
pub(crate) const ALT_SCREEN_PADDING: f32 = 10.0;

/// Terminal cell grid rect for an Alt Screen card occupying `card`.
pub(crate) fn alt_screen_grid_rect(card: Rect) -> Rect {
    let grid = card.shrink(ALT_SCREEN_PADDING);
    if grid.is_positive() { grid } else { card }
}

/// Paints the rounded Alt Screen container: opaque terminal background, glass border and
/// a top specular highlight.
pub(crate) fn paint_alt_screen_card(painter: &egui::Painter, card: Rect, theme: &MaterialTheme) {
    painter.rect(
        card,
        CornerRadius::same(ALT_SCREEN_RADIUS),
        theme.background,
        Stroke::new(1.0, theme.glass_border),
        StrokeKind::Inside,
    );
    paint_specular_highlight(
        painter,
        card,
        f32::from(ALT_SCREEN_RADIUS),
        theme.glass_highlight,
    );
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

    // Rounded Liquid Glass card that hosts the full-screen TUI (vim, htop, agy, ...).
    paint_alt_screen_card(ui.painter(), rect, theme);

    // Cells are painted strictly inside the inset grid rect, so neither cell backgrounds
    // nor the cursor can square off the rounded card corners.
    let grid_rect = alt_screen_grid_rect(rect);
    let painter = ui.painter_at(grid_rect);

    let grid = pane.engine.alt_screen();
    let (cursor_row, cursor_col) = grid.cursor();
    let origin = grid_rect.min;

    for r in 0..grid.rows() {
        let row = grid.row_cells(r);
        let y = origin.y + (r as f32) * cell_h;
        if y > grid_rect.bottom() {
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

    fn rect_shapes(out: &egui::FullOutput) -> Vec<(egui::epaint::RectShape, Rect)> {
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Rect(r) => Some((r.clone(), c.clip_rect)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tab_pills_are_fully_rounded_and_never_clipped_by_workspace_edge() {
        let ctx = Context::default();
        let cfg = AppConfig::default();
        let theme = MaterialTheme::from_config(&cfg);
        theme.apply_to_egui(&ctx, &cfg);

        let bar_rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 46.0));
        let strip_rect = Rect::from_min_max(Pos2::new(80.0, 0.0), Pos2::new(760.0, 46.0));
        let tabs = vec![
            TabPillInfo {
                title: "Material-Terminal".into(),
                badge: "zsh".into(),
                is_active: false,
            },
            TabPillInfo {
                title: "▶ antigravity".into(),
                badge: "zsh".into(),
                is_active: true,
            },
            TabPillInfo {
                title: "▶ a-really-long-running-interactive-cli-name".into(),
                badge: "bash".into(),
                is_active: false,
            },
        ];

        let mut pill_rects = Vec::new();
        let mut out = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
                ..Default::default()
            },
            |ui| {
                ui.set_clip_rect(bar_rect);
                pill_rects = render_tab_strip(ui, strip_rect, &tabs, &theme).pill_rects;
            },
        );
        out.textures_delta.clear();

        assert_eq!(pill_rects.len(), 3);
        let shapes = rect_shapes(&out);
        for pill in &pill_rects {
            assert!(
                pill.top() >= bar_rect.top() + 6.0 && pill.bottom() <= bar_rect.bottom() - 6.0,
                "tab pill {pill:?} must keep >= 6px clearance inside the titlebar {bar_rect:?}"
            );
            assert!((pill.height() - TAB_PILL_HEIGHT).abs() < 0.5);
            // Long running-command titles are ellipsized, keeping pills compact.
            assert!(pill.width() < 260.0, "pill {pill:?} must stay compact");
        }

        // The active pill is painted as one rounded shape whose clip rect fully contains it
        // (no flat/sliced bottom edge).
        let active = pill_rects[1];
        let (shape, clip) = shapes
            .iter()
            .find(|(r, _)| r.rect == active)
            .expect("active tab pill must be painted as a rect shape");
        assert_eq!(shape.corner_radius, CornerRadius::same(TAB_PILL_RADIUS));
        assert!(
            clip.contains_rect(active.expand(0.5)),
            "active pill {active:?} must not be clipped (clip {clip:?})"
        );

        // No square (zero-radius) box is ever painted over any tab pill.
        for (r, _) in &shapes {
            if pill_rects.iter().any(|p| p.intersects(r.rect)) {
                assert!(
                    r.corner_radius != CornerRadius::ZERO,
                    "square shape {:?} painted over a rounded tab pill",
                    r.rect
                );
            }
        }
    }

    #[test]
    fn alt_screen_card_is_rounded_and_grid_stays_inside_corners() {
        let ctx = Context::default();
        let cfg = AppConfig::default();
        let theme = MaterialTheme::from_config(&cfg);
        theme.apply_to_egui(&ctx, &cfg);

        let card = Rect::from_min_size(Pos2::new(18.0, 58.0), Vec2::new(1124.0, 690.0));
        let mut out = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
                ..Default::default()
            },
            |ui| {
                paint_alt_screen_card(ui.painter(), card, &theme);
            },
        );
        out.textures_delta.clear();

        let shapes = rect_shapes(&out);
        let (shape, _) = shapes
            .iter()
            .find(|(r, _)| r.rect == card)
            .expect("alt screen card must be painted");
        assert_eq!(shape.corner_radius, CornerRadius::same(ALT_SCREEN_RADIUS));
        assert!(
            shape.stroke.width > 0.0,
            "alt screen card must have a glass border"
        );

        // Every corner of the cell grid lies inside the rounded arc of the card.
        let grid = alt_screen_grid_rect(card);
        assert!(card.contains_rect(grid));
        let r = f32::from(ALT_SCREEN_RADIUS);
        let arc_centers = [
            Pos2::new(card.left() + r, card.top() + r),
            Pos2::new(card.right() - r, card.top() + r),
            Pos2::new(card.left() + r, card.bottom() - r),
            Pos2::new(card.right() - r, card.bottom() - r),
        ];
        let grid_corners = [
            grid.left_top(),
            grid.right_top(),
            grid.left_bottom(),
            grid.right_bottom(),
        ];
        for (corner, center) in grid_corners.iter().zip(arc_centers) {
            assert!(
                corner.distance(center) <= r,
                "grid corner {corner:?} must sit inside the rounded card corner"
            );
        }
    }

    #[test]
    fn ellipsize_truncates_long_titles() {
        assert_eq!(ellipsize("short", 24), "short");
        let long = ellipsize("▶ a-really-long-running-interactive-cli-name", 24);
        assert_eq!(long.chars().count(), 24);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn split_rects_are_strictly_disjoint_for_both_axes() {
        let root = Rect::from_min_size(Pos2::new(0.0, 46.0), Vec2::new(1160.0, 714.0));

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
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
            ..Default::default()
        };

        let mut captured_timeline_rect = Rect::NOTHING;
        let mut captured_input_rect = Rect::NOTHING;

        let mut out = ctx.run_ui(raw.take(), |ui| {
            let inner = Rect::from_min_size(Pos2::new(18.0, 58.0), Vec2::new(1124.0, 690.0));
            let input_h = editor
                .desired_height(ui, cfg.font.size, inner.width())
                .min((inner.height() * 0.48).max(78.0));
            let gap = 10.0;
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
                        "zsh",
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });

        out.textures_delta.clear();

        assert!(!captured_timeline_rect.intersects(captured_input_rect));
        assert!(captured_input_rect.height() >= 76.0 && captured_input_rect.height() <= 110.0);
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
        let input_rect = Rect::from_min_size(Pos2::new(18.0, 640.0), Vec2::new(1124.0, 96.0));

        let raw1 = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
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
                        "zsh",
                        false,
                        None,
                        &[],
                        false,
                    );
                },
            );
        });
        out1.textures_delta.clear();

        let raw2 = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
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
                        "zsh",
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
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
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
                        "zsh",
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
