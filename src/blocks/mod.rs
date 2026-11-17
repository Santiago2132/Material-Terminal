//! CommandBlock data model, `vte::Perform` ANSI/OSC 133/OSC 7 parser,
//! Alternate Screen handoff, and non-blocking Git branch detector.

use crate::terminal::{AltScreenGrid, TextStyle, apply_sgr_params};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use vte::{Params, Parser, Perform};

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

const MAX_LINES_PER_BLOCK: usize = 8000;

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
    active_line_idx: usize,
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

    fn current_line_mut(&mut self) -> &mut OutputLine {
        if self.output_lines.is_empty() {
            self.output_lines.push(OutputLine::default());
            self.active_line_idx = 0;
        }
        if self.active_line_idx >= self.output_lines.len() {
            self.active_line_idx = self.output_lines.len() - 1;
        }
        &mut self.output_lines[self.active_line_idx]
    }

    fn push_char(&mut self, ch: char, style: TextStyle) {
        if ch.is_control() {
            return;
        }
        self.current_line_mut().write_char(ch, style);
    }

    fn newline(&mut self) {
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

/// Semantic lifecycle phase tracked from OSC 133 sequences.
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
        // Finalize any dangling running block if a new command is explicitly launched.
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

        if self.state.saw_osc133 {
            // Suppress ZLE/readline command echo until OSC 133;C arrives.
            self.state.osc_phase = OscPhase::InCommandEcho;
        } else {
            self.state.osc_phase = OscPhase::InExecution;
        }
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

    pub fn update_git_branch(
        &mut self,
        dir: &Path,
        block_id: Option<u64>,
        branch: Option<String>,
    ) {
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
        let kind = std::str::from_utf8(params[1]).unwrap_or("");
        // Handle both `133;D;0` (3 params) and `133;D` (2 params)
        let kind_trimmed = kind.trim();
        self.saw_osc133 = true;

        match kind_trimmed {
            "A" => {
                // If somehow still running without D, leave active block until D arrives.
                self.osc_phase = OscPhase::InPrompt;
            }
            "B" => {
                self.osc_phase = OscPhase::InCommandEcho;
            }
            "C" => {
                self.osc_phase = OscPhase::InExecution;
            }
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

        let decoded = percent_decode(path_part);
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
                let mode = params
                    .iter()
                    .next()
                    .and_then(|s| s.first().copied())
                    .unwrap_or(0);
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
                let mode = params
                    .iter()
                    .next()
                    .and_then(|s| s.first().copied())
                    .unwrap_or(0);
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

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Resolves the Git branch for `dir` rapidly by walking up ancestor `.git/HEAD` files.
pub fn detect_git_branch_sync(dir: &Path) -> Option<String> {
    let mut cur = Some(dir);
    while let Some(candidate) = cur {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return read_git_head(&dot_git.join("HEAD"));
        } else if dot_git.is_file() {
            // Worktree or submodule `.git` file pointing to `gitdir: ...`
            if let Ok(contents) = fs::read_to_string(&dot_git)
                && let Some(gitdir_line) = contents.lines().find(|l| l.starts_with("gitdir:"))
            {
                let rel = gitdir_line.trim_start_matches("gitdir:").trim();
                let gitdir_path = if Path::new(rel).is_absolute() {
                    PathBuf::from(rel)
                } else {
                    candidate.join(rel)
                };
                return read_git_head(&gitdir_path.join("HEAD"));
            }
        }
        cur = candidate.parent();
    }
    None
}

fn read_git_head(head_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(head_path).ok()?;
    let trimmed = raw.trim();
    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
        return Some(branch.to_string());
    }
    if let Some(other_ref) = trimmed.strip_prefix("ref: ") {
        return Some(
            other_ref
                .rsplit('/')
                .next()
                .unwrap_or(other_ref)
                .to_string(),
        );
    }
    if trimmed.len() >= 7 {
        Some(format!("@{}", &trimmed[..7]))
    } else {
        None
    }
}

/// Request sent to the background Git branch worker.
#[derive(Debug, Clone)]
pub struct GitBranchRequest {
    pub pane_id: u64,
    pub block_id: Option<u64>,
    pub dir: PathBuf,
}

/// Response sent back asynchronously to the UI thread.
#[derive(Debug, Clone)]
pub struct GitBranchResult {
    pub pane_id: u64,
    pub block_id: Option<u64>,
    pub dir: PathBuf,
    pub branch: Option<String>,
}

/// Non-blocking Git branch detector running on a dedicated worker thread.
pub struct AsyncGitDetector {
    req_tx: Sender<GitBranchRequest>,
    res_rx: Receiver<GitBranchResult>,
}

impl AsyncGitDetector {
    pub fn spawn(wake_cb: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        let (req_tx, req_rx) = unbounded::<GitBranchRequest>();
        let (res_tx, res_rx) = unbounded::<GitBranchResult>();

        let _ = thread::Builder::new()
            .name("git-branch-detector".to_string())
            .spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    let branch = detect_git_branch_sync(&req.dir);
                    if res_tx
                        .send(GitBranchResult {
                            pane_id: req.pane_id,
                            block_id: req.block_id,
                            dir: req.dir,
                            branch,
                        })
                        .is_err()
                    {
                        break;
                    }
                    if let Some(cb) = &wake_cb {
                        cb();
                    }
                }
            });

        Self { req_tx, res_rx }
    }

    pub fn request(&self, pane_id: u64, block_id: Option<u64>, dir: PathBuf) {
        let _ = self.req_tx.send(GitBranchRequest {
            pane_id,
            block_id,
            dir,
        });
    }

    pub fn poll_results(&self) -> Vec<GitBranchResult> {
        let mut results = Vec::new();
        while let Ok(r) = self.res_rx.try_recv() {
            results.push(r);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::TermColor;

    #[test]
    fn parses_osc133_prompt_echo_output_and_exit_code() {
        let mut engine = TerminalEngine::new(PathBuf::from("/tmp"), 80, 24, 100);
        // Shell emits initial prompt
        engine.process_bytes(b"\x1b]133;A\x07user@mac % \x1b]133;B\x07");

        let block_id = engine.start_command_block("echo hello".to_string(), None);
        // Shell echoes command, emits C, outputs colored text, then D;0
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

    #[test]
    fn detects_git_branch_async_in_current_repo() {
        let detector = AsyncGitDetector::spawn(None);
        let cwd = std::env::current_dir().unwrap();
        detector.request(1, None, cwd.clone());

        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let mut got = None;
        while Instant::now() < deadline {
            if let Some(res) = detector.poll_results().into_iter().next() {
                got = Some(res);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let res = got.expect("expected async git branch result");
        assert!(res.branch.is_some());
    }
}
