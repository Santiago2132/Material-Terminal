//! Integration tests for PTY spawning and zsh/bash OSC 133 shell hooks.

use super::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temp_state_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "material-pty-test-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn zsh_emits_osc133_lifecycle_markers_and_exit_code() {
    if !PathBuf::from("/bin/zsh").exists() {
        return;
    }
    let data_dir = temp_state_dir("zsh");
    let cfg = PtySpawnConfig {
        program: Some("/bin/zsh".to_string()),
        args: vec!["-i".to_string()],
        working_dir: std::env::temp_dir(),
        cols: 100,
        rows: 24,
        enable_osc133_hooks: true,
        extra_env: vec![("MATERIAL_SKIP_USER_RC".to_string(), "1".to_string())],
        custom_data_dir: Some(data_dir.clone()),
    };

    let session = PtySession::spawn(cfg, None).expect("spawn zsh PTY");

    let mut collected = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for ev in session.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                collected.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if collected.contains("\x1b]133;A") && collected.contains("\x1b]133;B") {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        collected.contains("\x1b]133;A") && collected.contains("\x1b]133;B"),
        "missing initial OSC 133;A/B in: {:?}",
        collected
    );

    collected.clear();
    session.send_command("echo MATERIAL_OK && false");

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for ev in session.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                collected.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if collected.contains("\x1b]133;D;1") {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    assert!(collected.contains("\x1b]133;C"), "missing OSC 133;C");
    assert!(collected.contains("MATERIAL_OK"), "missing command stdout");
    assert!(collected.contains("\x1b]133;D;1"), "missing OSC 133;D;1");

    drop(session);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn bash_emits_osc133_lifecycle_markers_and_exit_code() {
    if !PathBuf::from("/bin/bash").exists() {
        return;
    }
    let data_dir = temp_state_dir("bash");
    let cfg = PtySpawnConfig {
        program: Some("/bin/bash".to_string()),
        args: Vec::new(),
        working_dir: std::env::temp_dir(),
        cols: 100,
        rows: 24,
        enable_osc133_hooks: true,
        extra_env: vec![("MATERIAL_SKIP_USER_RC".to_string(), "1".to_string())],
        custom_data_dir: Some(data_dir.clone()),
    };

    let session = PtySession::spawn(cfg, None).expect("spawn bash PTY");

    let mut collected = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for ev in session.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                collected.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if collected.contains("\x1b]133;A") && collected.contains("\x1b]133;B") {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        collected.contains("\x1b]133;A") && collected.contains("\x1b]133;B"),
        "missing initial bash OSC 133;A/B"
    );

    collected.clear();
    session.send_command("echo BASH_OK");

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for ev in session.drain_events() {
            if let PtyEvent::Output(bytes) = ev {
                collected.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        if collected.contains("\x1b]133;D;0") {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    assert!(collected.contains("\x1b]133;C"), "missing bash OSC 133;C");
    assert!(collected.contains("BASH_OK"), "missing bash stdout");
    assert!(collected.contains("\x1b]133;D;0"), "missing bash OSC 133;D;0");

    drop(session);
    let _ = std::fs::remove_dir_all(data_dir);
}
