//! Unit tests for `TerminalEngine` OSC 133, carriage-return, and Alt Screen handoff.

use super::*;
use crate::blocks::CommandStatus;
use crate::terminal::TermColor;
use std::path::PathBuf;

#[test]
fn parses_osc133_prompt_echo_output_and_exit_code() {
    let mut engine = TerminalEngine::new(PathBuf::from("/tmp"), 80, 24, 100);
    engine.process_bytes(b"\x1b]133;A\x07user@mac % \x1b]133;B\x07");

    let block_id = engine.start_command_block("echo hello".to_string(), None);
    let events = engine.process_bytes(
        b"echo hello\r\n\x1b]133;C\x07\x1b[38;2;10;200;120m\x1b[1mhello\x1b[0m\r\n\x1b]133;D;0\x07\x1b]133;A\x07user@mac % \x1b]133;B\x07",
    );

    assert!(events.contains(&EngineEvent::BlockCompleted {
        block_id,
        history_row_id: None,
        exit_code: 0,
    }));
    let block = &engine.blocks()[0];
    assert_eq!(block.status, CommandStatus::Success(0));
    assert_eq!(block.plain_output_text(), "hello");
    let spans = block.visible_lines()[0].spans();
    assert_eq!(spans[0].text, "hello");
    assert_eq!(spans[0].style.fg, TermColor::Rgb(10, 200, 120));
    assert!(spans[0].style.bold);
}

#[test]
fn handles_carriage_return_progress_bars_and_alt_screen_handoff() {
    let mut engine = TerminalEngine::new(PathBuf::from("/tmp"), 80, 24, 100);
    engine.start_command_block("cargo build".to_string(), None);
    engine.process_bytes(b"\x1b]133;C\x07Compiling 10%\r\x1b[KCompiling 100%\r\n");

    assert_eq!(engine.blocks()[0].plain_output_text(), "Compiling 100%");

    let enter_events = engine.process_bytes(b"\x1b[?1049h\x1b[1;1H\x1b[32mHTOP");
    assert!(enter_events.contains(&EngineEvent::AltScreenEntered));
    assert!(engine.is_alt_screen());
    assert_eq!(engine.alt_screen().row_cells(0)[0].ch, 'H');

    let exit_events = engine.process_bytes(b"\x1b[?1049l\x1b]133;D;0\x07");
    assert!(exit_events.contains(&EngineEvent::AltScreenExited));
    assert!(!engine.is_alt_screen());
    assert_eq!(engine.blocks()[0].status, CommandStatus::Success(0));
}
