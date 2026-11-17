use material_terminal::blocks::{CommandStatus, EngineEvent, TerminalEngine};
use material_terminal::config::{AppConfig, BuiltinTheme};
use material_terminal::history::HistoryStore;
use material_terminal::pty::{PtyEvent, PtySession, PtySpawnConfig};
use material_terminal::terminal::TermColor;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("mat-int-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn live_zsh_pty_drives_command_blocks_truecolor_cwd_and_history() {
    if !PathBuf::from("/bin/zsh").exists() {
        return;
    }

    let state_dir = temp_dir("flow");
    let work_dir = state_dir.join("workspace");
    std::fs::create_dir_all(&work_dir).unwrap();
    let canonical_work = std::fs::canonicalize(&work_dir).unwrap();

    let cfg = AppConfig::default();
    assert_eq!(cfg.resolved_theme(), BuiltinTheme::Obsidian);

    let history = HistoryStore::open_at(&state_dir.join("history.sqlite")).unwrap();
    let pty = PtySession::spawn(
        PtySpawnConfig {
            program: Some("/bin/zsh".to_string()),
            args: vec!["-i".to_string()],
            working_dir: canonical_work.clone(),
            cols: 100,
            rows: 30,
            enable_osc133_hooks: true,
            extra_env: vec![("MATERIAL_SKIP_USER_RC".to_string(), "1".to_string())],
            custom_data_dir: Some(state_dir.clone()),
        },
        None,
    )
    .expect("spawn zsh");

    let mut engine = TerminalEngine::new(canonical_work.clone(), 100, 30, 100);

    // Wait for initial prompt readiness (OSC 133;B)
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while Instant::now() < deadline {
        for ev in pty.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                let raw = String::from_utf8_lossy(&bytes);
                if raw.contains("\x1b]133;B") {
                    ready = true;
                }
                engine.process_bytes(&bytes);
            }
        }
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready, "zsh never emitted initial OSC 133;B");

    // 1. Run a TrueColor command
    let cmd1 = r#"printf '\033[38;2;255;100;50mCOLOR_OK\033[0m\n'"#;
    let row1 = history
        .record_command(cmd1, engine.current_dir(), None)
        .unwrap();
    let b1 = engine.start_command_block(cmd1.to_string(), row1);
    pty.send_command(cmd1);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut b1_done = false;
    while Instant::now() < deadline && !b1_done {
        for ev in pty.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                for ee in engine.process_bytes(&bytes) {
                    if let EngineEvent::BlockCompleted {
                        block_id,
                        history_row_id,
                        exit_code,
                    } = ee
                    {
                        assert_eq!(block_id, b1);
                        assert_eq!(exit_code, 0);
                        if let Some(id) = history_row_id {
                            history.update_exit_code(id, exit_code).unwrap();
                        }
                        b1_done = true;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(b1_done, "first block never completed");
    assert_eq!(engine.blocks()[0].status, CommandStatus::Success(0));
    assert_eq!(engine.blocks()[0].plain_output_text(), "COLOR_OK");
    let spans = engine.blocks()[0].visible_lines()[0].spans();
    assert_eq!(spans[0].style.fg, TermColor::Rgb(255, 100, 50));

    // 2. Change directory and verify OSC 7 updates engine.current_dir() + non-zero exit code
    let sub_dir = canonical_work.join("nested");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let cmd2 = format!("cd {} && false", sub_dir.display());
    let row2 = history
        .record_command(&cmd2, engine.current_dir(), None)
        .unwrap();
    let b2 = engine.start_command_block(cmd2.clone(), row2);
    pty.send_command(&cmd2);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut b2_done = false;
    let mut saw_dir_change = false;
    while Instant::now() < deadline && (!b2_done || !saw_dir_change) {
        for ev in pty.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                for ee in engine.process_bytes(&bytes) {
                    match ee {
                        EngineEvent::BlockCompleted {
                            block_id,
                            history_row_id,
                            exit_code,
                        } => {
                            assert_eq!(block_id, b2);
                            assert_eq!(exit_code, 1);
                            if let Some(id) = history_row_id {
                                history.update_exit_code(id, exit_code).unwrap();
                            }
                            b2_done = true;
                        }
                        EngineEvent::DirChanged(new_dir) if new_dir == sub_dir => {
                            saw_dir_change = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(b2_done, "second block never completed");
    assert!(
        saw_dir_change,
        "OSC 7 never updated directory to {}",
        sub_dir.display()
    );
    assert_eq!(engine.blocks()[1].status, CommandStatus::Error(1));
    assert_eq!(engine.current_dir(), sub_dir);

    // 3. Verify SQLite history + ghost suggestion works for the recorded command
    let recent = history.recent_unique(10);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].exit_code, Some(1));
    assert_eq!(recent[1].exit_code, Some(0));
    assert_eq!(
        history.suggest_ghost_suffix("printf '\\033", engine.current_dir()),
        Some("[38;2;255;100;50mCOLOR_OK\\033[0m\\n'".to_string())
    );

    drop(pty);
    let _ = std::fs::remove_dir_all(state_dir);
}
