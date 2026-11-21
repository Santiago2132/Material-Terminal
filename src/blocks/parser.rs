//! `vte::Perform` ANSI/OSC 133/OSC 7 parser and `TerminalEngine` state machine.

use super::model::{CommandBlock, CommandStatus, OutputLine};
use crate::terminal::{AltScreenGrid, TextStyle, apply_sgr_params};
use std::path::{Path, PathBuf};
use vte::{Params, Parser, Perform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OscPhase {
    Output,
    InPrompt,
    InCommandEcho,
    InExecution,
}

/// High-level semantic events emitted by `TerminalEngine` during PTY byte parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    DirChanged(PathBuf),
    BlockCompleted {
        block_id: u64,
        history_row_id: Option<i64>,
        exit_code: i32,
    },
    AltScreenEntered,
    AltScreenExited,
    Bell,
}

/// Combines `vte::Parser`, block timeline state, OSC 133/7 tracking, and Alternate Screen Grid.
pub struct TerminalEngine {
    parser: Parser,
    state: EngineState,
}

struct EngineState {
    blocks: Vec<CommandBlock>,
    active_block_id: Option<u64>,
    current_dir: PathBuf,
    current_git_branch: Option<String>,
    alt_screen: AltScreenGrid,
    alt_screen_active: bool,
    osc_phase: OscPhase,
    saw_osc133: bool,
    style: TextStyle,
    next_block_id: u64,
    max_blocks: usize,
    pending_events: Vec<EngineEvent>,
}

impl TerminalEngine {
    pub fn new(initial_dir: PathBuf, cols: usize, rows: usize, max_blocks: usize) -> Self {
        Self {
            parser: Parser::new(),
            state: EngineState {
                blocks: Vec::new(),
                active_block_id: None,
                current_dir: initial_dir,
                current_git_branch: None,
                alt_screen: AltScreenGrid::new(cols, rows),
                alt_screen_active: false,
                osc_phase: OscPhase::Output,
                saw_osc133: false,
                style: TextStyle::default(),
                next_block_id: 1,
                max_blocks: max_blocks.max(10),
                pending_events: Vec::new(),
            },
        }
    }

    pub fn set_max_blocks(&mut self, max_blocks: usize) {
        self.state.max_blocks = max_blocks.max(10);
        self.state.trim_blocks();
    }

    /// Registers a newly submitted command from the Input Editor and returns its block ID.
    pub fn start_command_block(
        &mut self,
        command_text: String,
        history_row_id: Option<i64>,
    ) -> u64 {
        if let Some(prev_id) = self.state.active_block_id.take()
            && let Some(prev) = self.state.blocks.iter_mut().find(|b| b.id == prev_id)
            && prev.status.is_running()
        {
            prev.status = CommandStatus::Success(0);
            prev.duration_ms = Some(prev.started_at.elapsed().as_millis() as u64);
        }

        let id = self.state.next_block_id;
        self.state.next_block_id += 1;

        let block = CommandBlock::new(
            id,
            command_text,
            self.state.current_dir.clone(),
            self.state.current_git_branch.clone(),
            history_row_id,
        );
        self.state.blocks.push(block);
        self.state.trim_blocks();
        self.state.active_block_id = Some(id);
        self.state.style.reset();

        self.state.osc_phase = if self.state.saw_osc133 {
            OscPhase::InCommandEcho
        } else {
            OscPhase::InExecution
        };
        id
    }

    /// Parses raw PTY bytes and returns semantic events triggered by this chunk.
    pub fn process_bytes(&mut self, bytes: &[u8]) -> Vec<EngineEvent> {
        self.parser.advance(&mut self.state, bytes);
        std::mem::take(&mut self.state.pending_events)
    }

    pub fn blocks(&self) -> &[CommandBlock] {
        &self.state.blocks
    }

    pub fn blocks_mut(&mut self) -> &mut Vec<CommandBlock> {
        &mut self.state.blocks
    }

    pub fn clear_blocks(&mut self) {
        if let Some(active_id) = self.state.active_block_id {
            self.state.blocks.retain(|b| b.id == active_id);
        } else {
            self.state.blocks.clear();
        }
    }

    pub fn active_block_id(&self) -> Option<u64> {
        self.state.active_block_id
    }

    pub fn has_running_block(&self) -> bool {
        self.state
            .active_block_id
            .and_then(|id| self.state.blocks.iter().find(|b| b.id == id))
            .is_some_and(|b| b.status.is_running())
    }

    pub fn current_dir(&self) -> &Path {
        &self.state.current_dir
    }

    pub fn current_git_branch(&self) -> Option<&str> {
        self.state.current_git_branch.as_deref()
    }

    pub fn update_git_branch(&mut self, dir: &Path, block_id: Option<u64>, branch: Option<String>) {
        if self.state.current_dir == dir {
            self.state.current_git_branch = branch.clone();
        }
        if let Some(id) = block_id
            && let Some(block) = self.state.blocks.iter_mut().find(|b| b.id == id)
        {
            block.git_branch = branch;
        }
    }

    pub fn is_alt_screen(&self) -> bool {
        self.state.alt_screen_active
    }

    pub fn alt_screen(&self) -> &AltScreenGrid {
        &self.state.alt_screen
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.state.alt_screen.resize(cols, rows);
    }
}

impl EngineState {
    fn trim_blocks(&mut self) {
        while self.blocks.len() > self.max_blocks {
            if Some(self.blocks[0].id) == self.active_block_id {
                break;
            }
            self.blocks.remove(0);
        }
    }

    fn active_block_mut(&mut self) -> Option<&mut CommandBlock> {
        let id = self.active_block_id?;
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    fn should_capture_output(&self) -> bool {
        match self.osc_phase {
            OscPhase::InPrompt | OscPhase::InCommandEcho => false,
            OscPhase::InExecution => true,
            OscPhase::Output => self.active_block_id.is_some(),
        }
    }

    fn handle_osc133(&mut self, params: &[&[u8]]) {
        if params.len() < 2 {
            return;
        }
        let kind = std::str::from_utf8(params[1]).unwrap_or("").trim();
        self.saw_osc133 = true;

        match kind {
            "A" => self.osc_phase = OscPhase::InPrompt,
            "B" => self.osc_phase = OscPhase::InCommandEcho,
            "C" => self.osc_phase = OscPhase::InExecution,
            "D" => {
                let code = params
                    .get(2)
                    .and_then(|raw| std::str::from_utf8(raw).ok())
                    .and_then(|s| s.trim().parse::<i32>().ok())
                    .unwrap_or(0);
                self.complete_active_block(code);
                self.osc_phase = OscPhase::Output;
            }
            _ => {}
        }
    }

    fn complete_active_block(&mut self, exit_code: i32) {
        if let Some(id) = self.active_block_id.take()
            && let Some(block) = self.blocks.iter_mut().find(|b| b.id == id)
        {
            block.status = if exit_code == 0 {
                CommandStatus::Success(0)
            } else {
                CommandStatus::Error(exit_code)
            };
            block.duration_ms = Some(block.started_at.elapsed().as_millis() as u64);
            self.pending_events.push(EngineEvent::BlockCompleted {
                block_id: block.id,
                history_row_id: block.history_row_id,
                exit_code,
            });
        }
    }

    fn handle_osc7(&mut self, raw_url: &[u8]) {
        let Ok(url_str) = std::str::from_utf8(raw_url) else {
            return;
        };
        let path_part = if let Some(rest) = url_str.strip_prefix("file://") {
            match rest.find('/') {
                Some(idx) => &rest[idx..],
                None => return,
            }
        } else {
            url_str
        };

        let decoded = super::osc::percent_decode(path_part);
        let new_path = PathBuf::from(decoded);
        if new_path != self.current_dir && new_path.is_dir() {
            self.current_dir = new_path.clone();
            self.pending_events.push(EngineEvent::DirChanged(new_path));
        }
    }
}

impl Perform for EngineState {
    fn print(&mut self, c: char) {
        if self.alt_screen_active {
            self.alt_screen.print_char(c);
            return;
        }
        if !self.should_capture_output() {
            return;
        }
        let style = self.style;
        if let Some(block) = self.active_block_mut() {
            block.push_char(c, style);
        }
    }

    fn execute(&mut self, byte: u8) {
        if byte == 0x07 {
            self.pending_events.push(EngineEvent::Bell);
            return;
        }
        if self.alt_screen_active {
            self.alt_screen.execute(byte);
            return;
        }
        if !self.should_capture_output() {
            return;
        }
        let style = self.style;
        if let Some(block) = self.active_block_mut() {
            match byte {
                b'\n' | 0x0b | 0x0c => block.newline(),
                b'\r' => block.current_line_mut().carriage_return(),
                0x08 => block.current_line_mut().backspace(),
                b'\t' => block.current_line_mut().tab(style),
                _ => {}
            }
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }
        match params[0] {
            b"133" => self.handle_osc133(params),
            b"7" if params.len() >= 2 => self.handle_osc7(params[1]),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        if intermediates == *b"?" && (action == 'h' || action == 'l') {
            let entering = action == 'h';
            let mut toggled_alt = false;
            for sub in params.iter() {
                if matches!(sub.first().copied(), Some(1049 | 1047 | 47)) {
                    toggled_alt = true;
                    if entering && !self.alt_screen_active {
                        self.alt_screen_active = true;
                        self.alt_screen
                            .reset(self.alt_screen.cols(), self.alt_screen.rows());
                        self.pending_events.push(EngineEvent::AltScreenEntered);
                    } else if !entering && self.alt_screen_active {
                        self.alt_screen_active = false;
                        self.pending_events.push(EngineEvent::AltScreenExited);
                    }
                }
            }
            if toggled_alt {
                return;
            }
        }

        if self.alt_screen_active {
            self.alt_screen.csi_dispatch(params, intermediates, action);
            return;
        }
        if !intermediates.is_empty() || !self.should_capture_output() {
            return;
        }

        match action {
            'm' => apply_sgr_params(&mut self.style, params),
            'K' => {
                let mode = params.iter().next().and_then(|s| s.first().copied()).unwrap_or(0);
                if let Some(block) = self.active_block_mut() {
                    block.current_line_mut().erase_in_line(mode);
                }
            }
            'G' => {
                let col = params
                    .iter()
                    .next()
                    .and_then(|s| s.first().copied())
                    .unwrap_or(1)
                    .saturating_sub(1) as usize;
                if let Some(block) = self.active_block_mut() {
                    block.current_line_mut().set_col(col);
                }
            }
            'J' => {
                let mode = params.iter().next().and_then(|s| s.first().copied()).unwrap_or(0);
                if (mode == 2 || mode == 3)
                    && let Some(block) = self.active_block_mut()
                {
                    block.output_lines.clear();
                    block.output_lines.push(OutputLine::default());
                    block.active_line_idx = 0;
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if self.alt_screen_active {
            self.alt_screen.esc_dispatch(intermediates, byte);
        }
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
