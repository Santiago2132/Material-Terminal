use egui::{Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
use material_terminal::app::{MaterialTerminalApp, SplitAxis};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("mat-gui-e2e-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_one_frame(
    ctx: &Context,
    app: &mut MaterialTerminalApp,
    events: Vec<Event>,
) -> egui::FullOutput {
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160.0, 760.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.render_workspace(ui);
    });
    out.textures_delta.clear();
    out
}

#[test]
fn full_workspace_renders_top_bar_at_top_accepts_typing_and_runs_real_shell_command() {
    if !PathBuf::from("/bin/zsh").exists() {
        return;
    }

    let tmp = temp_dir("full");
    let cfg_path = tmp.join("cfg").join("config.toml");
    let data_dir = tmp.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let ctx = Context::default();
    let mut app = MaterialTerminalApp::new_with_paths(&ctx, cfg_path, Some(data_dir), true);

    // Wait briefly for initial zsh prompt readiness
    let wait_start = Instant::now();
    while wait_start.elapsed() < Duration::from_secs(3) {
        let out = run_one_frame(&ctx, &mut app, Vec::new());
        if app.tabs.len() == 1
            && app.last_bar_rect.height() >= 44.0
            && app.last_workspace_rect.height() > 650.0
        {
            // Verify top bar is strictly pinned to top (y=0..46), NOT floating in middle!
            assert!(
                app.last_bar_rect.top() <= 1.0 && app.last_bar_rect.bottom() <= 48.0,
                "top bar must stay at top, got {:?}",
                app.last_bar_rect
            );
            assert!(
                app.last_workspace_rect.top() >= 45.0 && app.last_workspace_rect.bottom() >= 755.0,
                "workspace must fill below top bar, got {:?}",
                app.last_workspace_rect
            );

            // Verify input dock context bar renders near bottom (y >= 640)
            let dock_text = out.shapes.iter().find_map(|c| {
                if let egui::epaint::Shape::Text(t) = &c.shape
                    && t.galley.text().contains("OSC 133")
                {
                    return Some(t.pos);
                }
                None
            });
            let dock_pos = dock_text.expect("input dock must be rendered");
            assert!(
                dock_pos.y >= 640.0,
                "input dock must be anchored at bottom, got y={}",
                dock_pos.y
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    // Give zsh a brief moment to emit initial OSC 133;B before typing command
    std::thread::sleep(Duration::from_millis(250));
    run_one_frame(&ctx, &mut app, Vec::new());

    // Simulate user typing "echo GUI_LIVE_OK"
    run_one_frame(
        &ctx,
        &mut app,
        vec![Event::Text("echo GUI_LIVE_OK".to_string())],
    );
    let out_typed = run_one_frame(&ctx, &mut app, Vec::new());
    let typed_pos = out_typed.shapes.iter().find_map(|c| {
        if let egui::epaint::Shape::Text(t) = &c.shape
            && t.galley.text() == "echo GUI_LIVE_OK"
        {
            return Some(t.pos);
        }
        None
    });
    let typed_pos = typed_pos.expect("typed command text must be visibly painted in input editor");
    assert!(
        typed_pos.y >= 660.0 && typed_pos.y <= 750.0,
        "typed text must appear inside bottom input dock, got y={}",
        typed_pos.y
    );

    // Simulate pressing Enter to submit command to live zsh PTY
    run_one_frame(
        &ctx,
        &mut app,
        vec![Event::Key {
            key: Key::Enter,
            physical_key: Some(Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );

    let mut saw_output_block = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let out = run_one_frame(&ctx, &mut app, Vec::new());
        let has_block_out = out.shapes.iter().any(|c| {
            matches!(&c.shape, egui::epaint::Shape::Text(t) if t.galley.text().contains("GUI_LIVE_OK"))
        });
        let pid = app.active_pane_id().unwrap();
        let pane = app.panes.get(&pid).unwrap();
        if has_block_out
            && !pane.engine.blocks().is_empty()
            && !pane.engine.blocks()[0].status.is_running()
        {
            saw_output_block = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        saw_output_block,
        "command output 'GUI_LIVE_OK' must complete and render inside CommandBlock"
    );

    // Split vertically and ensure both panes render side-by-side cleanly
    app.split_focused_pane(SplitAxis::Vertical);
    let out_split = run_one_frame(&ctx, &mut app, Vec::new());
    let dock_positions: Vec<Pos2> = out_split
        .shapes
        .iter()
        .filter_map(|c| {
            if let egui::epaint::Shape::Text(t) = &c.shape
                && t.galley.text().contains("OSC 133")
            {
                return Some(t.pos);
            }
            None
        })
        .collect();
    assert_eq!(dock_positions.len(), 2, "expected two split input docks");
    assert!(
        (dock_positions[0].x - dock_positions[1].x).abs() > 300.0,
        "split input docks must be side-by-side without overlapping: {:?}",
        dock_positions
    );

    // Open search bar + toast + command palette and verify layout remains intact and deadlock-free
    app.show_toast("Theme: Obsidian".to_string());
    app.toggle_search();
    run_one_frame(&ctx, &mut app, Vec::new());
    assert!(app.last_workspace_rect.height() > 550.0);

    app.palette.toggle();
    let out_palette = run_one_frame(&ctx, &mut app, Vec::new());
    assert!(out_palette.shapes.iter().any(|c| {
        matches!(&c.shape, egui::epaint::Shape::Text(t) if t.galley.text().contains("Theme:"))
    }));

    drop(app);
    let _ = std::fs::remove_dir_all(tmp);
}
