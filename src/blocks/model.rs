//! CommandBlock, OutputLine, StyledSpan, and CommandStatus data structures.

use crate::terminal::TextStyle;
use std::path::PathBuf;
use std::time::Instant;

const MAX_LINES_PER_BLOCK: usize = 8000;

/// Lifecycle state of an executed command block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Running,
    Success(i32),
    Error(i32),
}

impl CommandStatus {
    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    pub fn exit_code(self) -> Option<i32> {
        match self {
            Self::Running => None,
            Self::Success(c) | Self::Error(c) => Some(c),
        }
    }
}

/// Coalesced ANSI-styled span for fast GPU text rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub style: TextStyle,
}

/// A single logical output line supporting `\r` overwrites, backspaces, and line erasure.
#[derive(Debug, Clone, Default)]
pub struct OutputLine {
    cells: Vec<(char, TextStyle)>,
    cursor: usize,
}

impl OutputLine {
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn write_char(&mut self, ch: char, style: TextStyle) {
        if self.cursor < self.cells.len() {
            self.cells[self.cursor] = (ch, style);
        } else {
            while self.cells.len() < self.cursor {
                self.cells.push((' ', TextStyle::default()));
            }
            self.cells.push((ch, style));
        }
        self.cursor += 1;
    }

    pub fn carriage_return(&mut self) {
        self.cursor = 0;
    }

    pub fn backspace(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn tab(&mut self, style: TextStyle) {
        let next = ((self.cursor / 8) + 1) * 8;
        while self.cursor < next {
            self.write_char(' ', style);
        }
    }

    pub fn set_col(&mut self, col: usize) {
        self.cursor = col;
    }

    pub fn erase_in_line(&mut self, mode: u16) {
        match mode {
            0 => {
                if self.cursor < self.cells.len() {
                    self.cells.truncate(self.cursor);
                }
            }
            1 => {
                let end = (self.cursor + 1).min(self.cells.len());
                for cell in &mut self.cells[..end] {
                    *cell = (' ', TextStyle::default());
                }
            }
            2 => {
                self.cells.clear();
                self.cursor = 0;
            }
            _ => {}
        }
    }

    /// Returns coalesced spans with identical `TextStyle` grouped together.
    pub fn spans(&self) -> Vec<StyledSpan> {
        if self.cells.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut cur_style = self.cells[0].1;
        let mut cur_text = String::new();
        for &(ch, st) in &self.cells {
            if st != cur_style {
                if !cur_text.is_empty() {
                    out.push(StyledSpan {
                        text: std::mem::take(&mut cur_text),
                        style: cur_style,
                    });
                }
                cur_style = st;
            }
            cur_text.push(ch);
        }
        if !cur_text.is_empty() {
            out.push(StyledSpan {
                text: cur_text,
                style: cur_style,
            });
        }
        out
    }

    pub fn plain_text(&self) -> String {
        self.cells.iter().map(|(ch, _)| *ch).collect()
    }
}

/// Structured Warp-style unit representing one command invocation and its ANSI output.
#[derive(Debug, Clone)]
pub struct CommandBlock {
    pub id: u64,
    pub working_dir: PathBuf,
    pub git_branch: Option<String>,
    pub command_text: String,
    pub output_lines: Vec<OutputLine>,
    pub status: CommandStatus,
    pub started_at: Instant,
    pub duration_ms: Option<u64>,
    pub is_collapsed: bool,
    pub history_row_id: Option<i64>,
    pub(crate) active_line_idx: usize,
}

impl CommandBlock {
    pub fn new(
        id: u64,
        command_text: String,
        working_dir: PathBuf,
        git_branch: Option<String>,
        history_row_id: Option<i64>,
    ) -> Self {
        Self {
            id,
            working_dir,
            git_branch,
            command_text,
            output_lines: vec![OutputLine::default()],
            status: CommandStatus::Running,
            started_at: Instant::now(),
            duration_ms: None,
            is_collapsed: false,
            history_row_id,
            active_line_idx: 0,
        }
    }

    pub(crate) fn current_line_mut(&mut self) -> &mut OutputLine {
        if self.output_lines.is_empty() {
            self.output_lines.push(OutputLine::default());
            self.active_line_idx = 0;
        }
        if self.active_line_idx >= self.output_lines.len() {
            self.active_line_idx = self.output_lines.len() - 1;
        }
        &mut self.output_lines[self.active_line_idx]
    }

    pub(crate) fn push_char(&mut self, ch: char, style: TextStyle) {
        if ch.is_control() {
            return;
        }
        self.current_line_mut().write_char(ch, style);
    }

    pub(crate) fn newline(&mut self) {
        if self.output_lines.len() >= MAX_LINES_PER_BLOCK {
            self.output_lines.remove(0);
            self.active_line_idx = self.active_line_idx.saturating_sub(1);
        }
        self.output_lines.push(OutputLine::default());
        self.active_line_idx = self.output_lines.len() - 1;
    }

    /// Returns trimmed plain-text output (stripping a single trailing blank line).
    pub fn plain_output_text(&self) -> String {
        let mut lines: Vec<String> = self
            .output_lines
            .iter()
            .map(OutputLine::plain_text)
            .collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Returns visible output lines (omitting an empty trailing cursor line).
    pub fn visible_lines(&self) -> &[OutputLine] {
        let mut len = self.output_lines.len();
        while len > 0 && self.output_lines[len - 1].is_empty() {
            len -= 1;
        }
        &self.output_lines[..len]
    }

    pub fn has_output(&self) -> bool {
        !self.visible_lines().is_empty()
    }

    pub fn formatted_duration(&self) -> String {
        let ms = match self.duration_ms {
            Some(ms) => ms,
            None => self.started_at.elapsed().as_millis() as u64,
        };
        if ms < 1000 {
            format!("{ms}ms")
        } else if ms < 60_000 {
            format!("{:.1}s", ms as f64 / 1000.0)
        } else {
            let mins = ms / 60_000;
            let secs = (ms % 60_000) / 1000;
            format!("{mins}m {secs}s")
        }
    }
}
