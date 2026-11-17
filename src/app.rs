//! Top-level application state, workspace tabs, recursive split-pane tree,
//! background event polling, global shortcuts, and `eframe::App` implementation.

use crate::blocks::{AsyncGitDetector, EngineEvent, TerminalEngine};
use crate::config::{AppConfig, BuiltinTheme, ConfigEvent, ConfigManager};
use crate::history::{HistoryEntry, HistoryStore};
use crate::pty::{PtyEvent, PtySession, PtySpawnConfig};
use crate::ui::block_view::BlockAction;
use crate::ui::command_palette::{CommandPaletteState, PaletteAction};
use crate::ui::input_editor::{InputAction, InputEditorState, compact_home_path};
use crate::ui::theme::{MaterialTheme, apply_configured_font, paint_liquid_backdrop};
use crate::ui::{render_notifications, render_pane_tree, render_search_bar, render_top_bar};
use egui::{CentralPanel, Context, Event, Frame, Id, Key, Modifiers, ViewportCommand};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Orientation of a split divider between two pane subtrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// Side-by-side panes separated by a vertical divider (`Cmd+D`).
    Vertical,
    /// Stacked panes separated by a horizontal divider (`Cmd+Shift+D`).
    Horizontal,
}

/// Binary tree representing split-pane layout within a workspace tab.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneNode {
    Leaf(u64),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    pub fn leaves(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<u64>) {
        match self {
            Self::Leaf(id) => out.push(*id),
            Self::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    /// Replaces `target_id` leaf with a split containing `target_id` and `new_id`.
    pub fn split_leaf(&mut self, target_id: u64, new_id: u64, axis: SplitAxis) -> bool {
        match self {
            Self::Leaf(id) if *id == target_id => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(target_id)),
                    second: Box::new(Self::Leaf(new_id)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target_id, new_id, axis)
                    || second.split_leaf(target_id, new_id, axis)
            }
        }
    }

    /// Removes `target_id` leaf and collapses its parent split, returning `None` if empty.
    pub fn remove_leaf(self, target_id: u64) -> Option<Self> {
        match self {
            Self::Leaf(id) => {
                if id == target_id {
                    None
                } else {
                    Some(Self::Leaf(id))
                }
            }
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let first_res = first.remove_leaf(target_id);
                let second_res = second.remove_leaf(target_id);
                match (first_res, second_res) {
                    (None, None) => None,
                    (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                    (Some(f), Some(s)) => Some(Self::Split {
                        axis,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                }
            }
        }
    }

    pub fn cycle_leaf(&self, current: u64, forward: bool) -> Option<u64> {
        let all = self.leaves();
        if all.len() <= 1 {
            return all.first().copied();
        }
        let pos = all.iter().position(|&id| id == current).unwrap_or(0);
        let next_idx = if forward {
            (pos + 1) % all.len()
        } else if pos == 0 {
            all.len() - 1
        } else {
            pos - 1
        };
        Some(all[next_idx])
    }
}

/// Independent terminal pane owning its PTY session, VTE block engine, and Input Editor.
pub struct PaneState {
    pub id: u64,
    pub pty: PtySession,
    pub engine: TerminalEngine,
    pub editor: InputEditorState,
    pub scroll_to_bottom: bool,
    pub exited: Option<i32>,
}

/// Workspace tab containing a split tree of panes.
pub struct TabState {
    pub id: u64,
    pub root: PaneNode,
    pub focused_pane: u64,
}

/// Floating block search state (`Cmd+F`).
#[derive(Debug, Default)]
pub struct SearchState {
    pub open: bool,
    pub query: String,
    pub active_match_idx: usize,
    pub just_opened: bool,
    pub scroll_to_active: bool,
}

/// Main Material Terminal application implementing `eframe::App`.
pub struct MaterialTerminalApp {
    pub config_manager: ConfigManager,
    pub config: AppConfig,
    pub config_error: Option<String>,
    pub toast: Option<(String, Instant)>,
    pub history: Option<HistoryStore>,
    pub recent_history: Vec<HistoryEntry>,
    pub git_detector: AsyncGitDetector,
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub panes: HashMap<u64, PaneState>,
    pub palette: CommandPaletteState,
    pub search: SearchState,
    clipboard: Option<arboard::Clipboard>,
    wake_cb: Arc<dyn Fn() + Send + Sync>,
    next_id: u64,
    last_applied_theme: Option<(String, f32, f32)>,
    last_applied_font_family: String,
}

impl MaterialTerminalApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let wake_cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            ctx.request_repaint();
        });

        let (config_manager, startup_err) =
            ConfigManager::load_or_create_default(Some(wake_cb.clone()));
        let config = config_manager.current();

        let history = HistoryStore::open_default().ok();
        let recent_history = history
            .as_ref()
            .map(|h| h.recent_unique(200))
            .unwrap_or_default();

        let git_detector = AsyncGitDetector::spawn(Some(wake_cb.clone()));
        let clipboard = arboard::Clipboard::new().ok();

        let mut app = Self {
            config_manager,
            config: config.clone(),
            config_error: startup_err,
            toast: None,
            history,
            recent_history,
            git_detector,
            tabs: Vec::new(),
            active_tab: 0,
            panes: HashMap::new(),
            palette: CommandPaletteState::default(),
            search: SearchState::default(),
            clipboard,
            wake_cb,
            next_id: 1,
            last_applied_theme: None,
            last_applied_font_family: config.font.family.clone(),
        };

        apply_configured_font(&cc.egui_ctx, &config.font.family);
        let theme = MaterialTheme::from_config(&config);
        theme.apply_to_egui(&cc.egui_ctx, &config);
        app.last_applied_theme = Some((
            config.theme.clone(),
            config.window.opacity,
            config.font.size,
        ));

        app.new_tab();
        app
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn spawn_pane(&mut self, initial_dir: PathBuf) -> Option<u64> {
        let pane_id = self.alloc_id();
        let pty_cfg = PtySpawnConfig {
            program: self.config.shell.program.clone(),
            args: self.config.shell.args.clone(),
            working_dir: initial_dir.clone(),
            cols: 100,
            rows: 32,
            enable_osc133_hooks: self.config.shell.enable_osc133_hooks,
            ..PtySpawnConfig::default()
        };

        match PtySession::spawn(pty_cfg, Some(self.wake_cb.clone())) {
            Ok(pty) => {
                let engine =
                    TerminalEngine::new(initial_dir.clone(), 100, 32, self.config.shell.max_blocks);
                self.git_detector.request(pane_id, None, initial_dir);
                self.panes.insert(
                    pane_id,
                    PaneState {
                        id: pane_id,
                        pty,
                        engine,
                        editor: InputEditorState::default(),
                        scroll_to_bottom: false,
                        exited: None,
                    },
                );
                Some(pane_id)
            }
            Err(err) => {
                self.show_toast(format!("Failed to spawn shell: {err}"));
                None
            }
        }
    }

    pub fn new_tab(&mut self) {
        let start_dir = self.active_pane_dir().unwrap_or_else(|| {
            dirs::home_dir()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("/"))
        });

        if let Some(pane_id) = self.spawn_pane(start_dir) {
            let tab_id = self.alloc_id();
            self.tabs.push(TabState {
                id: tab_id,
                root: PaneNode::Leaf(pane_id),
                focused_pane: pane_id,
            });
            self.active_tab = self.tabs.len() - 1;
        }
    }

    pub fn close_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(idx);
        for pid in tab.root.leaves() {
            self.panes.remove(&pid);
        }
        if self.tabs.is_empty() {
            self.new_tab();
        } else {
            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            self.focus_active_pane_editor();
        }
    }

    pub fn close_active_pane(&mut self, ctx: &Context) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        let target = tab.focused_pane;
        let root_copy = tab.root.clone();
        match root_copy.remove_leaf(target) {
            Some(new_root) => {
                self.panes.remove(&target);
                let next_focus = new_root.leaves().first().copied().unwrap_or(0);
                tab.root = new_root;
                tab.focused_pane = next_focus;
                self.focus_active_pane_editor();
            }
            None => {
                if self.tabs.len() <= 1 {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                } else {
                    self.close_tab_at(self.active_tab);
                }
            }
        }
    }

    pub fn split_focused_pane(&mut self, axis: SplitAxis) {
        let dir = self
            .active_pane_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));
        let Some(new_id) = self.spawn_pane(dir) else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let target = tab.focused_pane;
            if tab.root.split_leaf(target, new_id, axis) {
                tab.focused_pane = new_id;
            }
        }
    }

    pub fn active_pane_id(&self) -> Option<u64> {
        self.tabs.get(self.active_tab).map(|t| t.focused_pane)
    }

    pub fn active_pane_count(&self) -> usize {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.root.leaves().len())
            .unwrap_or(1)
    }

    fn active_pane_dir(&self) -> Option<PathBuf> {
        let pid = self.active_pane_id()?;
        self.panes
            .get(&pid)
            .map(|p| p.engine.current_dir().to_path_buf())
    }

    pub fn set_focused_pane(&mut self, pane_id: u64) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.focused_pane = pane_id;
        }
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.editor.request_focus = true;
        }
    }

    pub fn focus_active_pane_editor(&mut self) {
        if let Some(pid) = self.active_pane_id()
            && let Some(pane) = self.panes.get_mut(&pid)
        {
            pane.editor.request_focus = true;
        }
    }

    pub fn tab_title(&self, tab: &TabState) -> String {
        let Some(pane) = self.panes.get(&tab.focused_pane) else {
            return format!("Tab {}", tab.id);
        };
        if let Some(active_id) = pane.engine.active_block_id()
            && let Some(block) = pane.engine.blocks().iter().find(|b| b.id == active_id)
            && block.status.is_running()
        {
            let first = block
                .command_text
                .split_whitespace()
                .next()
                .unwrap_or("running");
            return format!("▶ {first}");
        }
        let dir = pane.engine.current_dir();
        dir.file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| compact_home_path(dir))
    }

    pub fn pane_shell_name(&self, pane_id: u64) -> String {
        let Some(pane) = self.panes.get(&pane_id) else {
            return "sh".to_string();
        };
        std::path::Path::new(pane.pty.shell_program())
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("sh")
            .to_string()
    }

    pub fn tab_shell_badge(&self, tab: &TabState) -> String {
        self.pane_shell_name(tab.focused_pane)
    }

    pub fn cycle_theme(&mut self) {
        let cur = self.config.resolved_theme();
        let all = BuiltinTheme::ALL;
        let idx = all.iter().position(|t| *t == cur).unwrap_or(0);
        let next = all[(idx + 1) % all.len()];
        match self.config_manager.set_theme(next) {
            Ok(new_cfg) => {
                self.config = new_cfg;
                self.config_error = None;
                self.show_toast(format!("Theme: {}", next.display_name()));
            }
            Err(err) => {
                self.config_error = Some(err);
            }
        }
        self.focus_active_pane_editor();
    }

    pub fn toggle_search(&mut self) {
        self.search.open = !self.search.open;
        if self.search.open {
            self.search.just_opened = true;
        } else {
            self.focus_active_pane_editor();
        }
    }

    pub fn show_toast(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now() + Duration::from_secs(3)));
    }

    fn copy_text(&mut self, ctx: &Context, text: String, label: &str) {
        ctx.copy_text(text.clone());
        if let Some(cb) = &mut self.clipboard {
            let _ = cb.set_text(text);
        }
        self.show_toast(format!("Copied {label} to clipboard"));
    }

    pub fn handle_block_action(&mut self, ctx: &Context, pane_id: u64, action: BlockAction) {
        match action {
            BlockAction::None => {}
            BlockAction::CopyCommand(cmd) => self.copy_text(ctx, cmd, "command"),
            BlockAction::CopyOutput(out) => self.copy_text(ctx, out, "output"),
            BlockAction::RerunCommand(cmd) => {
                self.execute_command_in_pane(pane_id, cmd);
            }
            BlockAction::ToggleCollapse(block_id) => {
                if let Some(pane) = self.panes.get_mut(&pane_id)
                    && let Some(block) = pane
                        .engine
                        .blocks_mut()
                        .iter_mut()
                        .find(|b| b.id == block_id)
                {
                    block.is_collapsed = !block.is_collapsed;
                }
            }
        }
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.editor.request_focus = true;
        }
    }

    pub fn handle_input_action(&mut self, pane_id: u64, action: InputAction) {
        match action {
            InputAction::None => {}
            InputAction::SubmitCommand(cmd) => {
                self.execute_command_in_pane(pane_id, cmd);
            }
            InputAction::SendStdinLine(line) => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    let mut bytes = line.into_bytes();
                    bytes.push(b'\r');
                    pane.pty.write_bytes(&bytes);
                    pane.scroll_to_bottom = true;
                }
            }
            InputAction::SendInterrupt => {
                if let Some(pane) = self.panes.get(&pane_id) {
                    pane.pty.send_interrupt();
                }
            }
            InputAction::SendEof => {
                if let Some(pane) = self.panes.get(&pane_id) {
                    pane.pty.write_bytes(&[0x04]);
                }
            }
        }
    }

    fn execute_command_in_pane(&mut self, pane_id: u64, command: String) {
        let Some(pane) = self.panes.get_mut(&pane_id) else {
            return;
        };

        let cwd = pane.engine.current_dir().to_path_buf();
        let row_id = self
            .history
            .as_ref()
            .and_then(|h| h.record_command(&command, &cwd, None).ok().flatten());

        let block_id = pane.engine.start_command_block(command.clone(), row_id);
        pane.pty.send_command(&command);
        pane.scroll_to_bottom = true;
        pane.editor.request_focus = true;

        self.git_detector.request(pane_id, Some(block_id), cwd);

        if let Some(h) = &self.history {
            self.recent_history = h.recent_unique(200);
        }
    }

    fn apply_palette_action(&mut self, action: PaletteAction, ctx: &Context) {
        match action {
            PaletteAction::SetTheme(theme) => match self.config_manager.set_theme(theme) {
                Ok(new_cfg) => {
                    self.config = new_cfg;
                    self.config_error = None;
                    self.show_toast(format!("Theme switched to {}", theme.display_name()));
                }
                Err(err) => {
                    self.config_error = Some(err);
                }
            },
            PaletteAction::SplitVertical => self.split_focused_pane(SplitAxis::Vertical),
            PaletteAction::SplitHorizontal => self.split_focused_pane(SplitAxis::Horizontal),
            PaletteAction::NewTab => self.new_tab(),
            PaletteAction::ClosePane => self.close_active_pane(ctx),
            PaletteAction::ClearBlocks => {
                if let Some(pid) = self.active_pane_id()
                    && let Some(pane) = self.panes.get_mut(&pid)
                {
                    pane.engine.clear_blocks();
                }
            }
            PaletteAction::ToggleSearch => self.toggle_search(),
            PaletteAction::CollapseAllBlocks => {
                if let Some(pid) = self.active_pane_id()
                    && let Some(pane) = self.panes.get_mut(&pid)
                {
                    for b in pane.engine.blocks_mut() {
                        b.is_collapsed = true;
                    }
                }
            }
            PaletteAction::ExpandAllBlocks => {
                if let Some(pid) = self.active_pane_id()
                    && let Some(pane) = self.panes.get_mut(&pid)
                {
                    for b in pane.engine.blocks_mut() {
                        b.is_collapsed = false;
                    }
                }
            }
            PaletteAction::OpenConfig => self.open_config_file(ctx),
            PaletteAction::RunHistoryCommand(cmd) => {
                if let Some(pid) = self.active_pane_id() {
                    self.execute_command_in_pane(pid, cmd);
                }
            }
        }
    }

    fn open_config_file(&mut self, ctx: &Context) {
        let path = self.config_manager.path().to_path_buf();
        #[cfg(target_os = "macos")]
        let cmd_res = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(target_os = "linux")]
        let cmd_res = std::process::Command::new("xdg-open").arg(&path).spawn();
        #[cfg(target_os = "windows")]
        let cmd_res = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&path)
            .spawn();

        match cmd_res {
            Ok(_) => self.show_toast(format!("Opened {}", path.display())),
            Err(_) => {
                self.copy_text(ctx, path.display().to_string(), "config path");
            }
        }
    }

    fn poll_background_events(&mut self) {
        for ev in self.config_manager.poll_events() {
            match ev {
                ConfigEvent::Reloaded(new_cfg) => {
                    let max_blocks = new_cfg.shell.max_blocks;
                    let theme_name = new_cfg.resolved_theme().display_name();
                    self.config = *new_cfg;
                    self.config_error = None;
                    for pane in self.panes.values_mut() {
                        pane.engine.set_max_blocks(max_blocks);
                    }
                    self.show_toast(format!("Config reloaded · {theme_name}"));
                }
                ConfigEvent::ReloadError(err) => {
                    self.config_error = Some(err);
                }
            }
        }

        let pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        for pid in pane_ids {
            let events = if let Some(pane) = self.panes.get(&pid) {
                pane.pty.drain_events()
            } else {
                continue;
            };

            let mut dir_updates = Vec::new();
            let mut completed_updates = Vec::new();

            if let Some(pane) = self.panes.get_mut(&pid) {
                for ev in events {
                    match ev {
                        PtyEvent::Output(bytes) => {
                            let engine_events = pane.engine.process_bytes(&bytes);
                            pane.scroll_to_bottom = true;
                            for ee in engine_events {
                                match ee {
                                    EngineEvent::DirChanged(new_dir) => {
                                        dir_updates.push(new_dir);
                                    }
                                    EngineEvent::BlockCompleted {
                                        history_row_id,
                                        exit_code,
                                        ..
                                    } => {
                                        completed_updates.push((history_row_id, exit_code));
                                        pane.editor.request_focus = true;
                                    }
                                    EngineEvent::AltScreenEntered
                                    | EngineEvent::AltScreenExited
                                    | EngineEvent::Bell => {}
                                }
                            }
                        }
                        PtyEvent::Exited(code) => {
                            pane.exited = Some(code.unwrap_or(0));
                        }
                        PtyEvent::Error(err) => {
                            self.toast = Some((err, Instant::now() + Duration::from_secs(4)));
                        }
                    }
                }
            }

            for new_dir in dir_updates {
                self.git_detector.request(pid, None, new_dir);
            }
            if let Some(history) = &self.history {
                for (row_id_opt, exit_code) in completed_updates {
                    if let Some(row_id) = row_id_opt {
                        let _ = history.update_exit_code(row_id, exit_code);
                    }
                }
            }
        }

        for git_res in self.git_detector.poll_results() {
            if let Some(pane) = self.panes.get_mut(&git_res.pane_id) {
                pane.engine
                    .update_git_branch(&git_res.dir, git_res.block_id, git_res.branch);
            }
        }
    }

    fn handle_global_shortcuts(&mut self, ctx: &Context) {
        let kb = &self.config.keybindings;
        let palette_sc = parse_shortcut(&kb.command_palette, Modifiers::COMMAND, Key::K);
        // Command Palette: configured shortcut (default Cmd+K) or Ctrl+Shift+P
        let toggle_palette = ctx.input_mut(|i| {
            i.consume_key(palette_sc.0, palette_sc.1)
                || i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::P)
        });
        if toggle_palette {
            self.palette.toggle();
            return;
        }

        if self.palette.open {
            return;
        }

        let new_tab_sc = parse_shortcut(&kb.new_tab, Modifiers::COMMAND, Key::T);
        let close_tab_sc = parse_shortcut(&kb.close_tab, Modifiers::COMMAND, Key::W);
        let split_h_sc = parse_shortcut(
            &kb.split_horizontal,
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::D,
        );
        let split_v_sc = parse_shortcut(&kb.split_vertical, Modifiers::COMMAND, Key::D);
        let search_sc = parse_shortcut(&kb.search, Modifiers::COMMAND, Key::F);
        let clear_sc = parse_shortcut(&kb.clear_blocks, Modifiers::COMMAND, Key::L);
        let config_sc = parse_shortcut(&kb.open_config, Modifiers::COMMAND, Key::Comma);

        // Check shortcuts with Shift first so Cmd+Shift+D matches before Cmd+D.
        if ctx.input_mut(|i| i.consume_key(split_h_sc.0, split_h_sc.1)) {
            self.split_focused_pane(SplitAxis::Horizontal);
        } else if ctx.input_mut(|i| i.consume_key(split_v_sc.0, split_v_sc.1)) {
            self.split_focused_pane(SplitAxis::Vertical);
        } else if ctx.input_mut(|i| i.consume_key(new_tab_sc.0, new_tab_sc.1)) {
            self.new_tab();
        } else if ctx.input_mut(|i| i.consume_key(close_tab_sc.0, close_tab_sc.1)) {
            self.close_active_pane(ctx);
        } else if ctx.input_mut(|i| i.consume_key(search_sc.0, search_sc.1)) {
            self.toggle_search();
        } else if ctx.input_mut(|i| i.consume_key(clear_sc.0, clear_sc.1)) {
            if let Some(pid) = self.active_pane_id()
                && let Some(pane) = self.panes.get_mut(&pid)
            {
                pane.engine.clear_blocks();
            }
        } else if ctx.input_mut(|i| i.consume_key(config_sc.0, config_sc.1)) {
            self.open_config_file(ctx);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::CloseBracket)) {
            self.cycle_pane_focus(true);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::OpenBracket)) {
            self.cycle_pane_focus(false);
        } else {
            // Cmd+1..9 direct tab switch
            let num_keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            for (idx, k) in num_keys.into_iter().enumerate() {
                if idx < self.tabs.len() && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k))
                {
                    self.active_tab = idx;
                    self.focus_active_pane_editor();
                    break;
                }
            }
        }
    }

    fn cycle_pane_focus(&mut self, forward: bool) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if let Some(next_id) = tab.root.cycle_leaf(tab.focused_pane, forward) {
            tab.focused_pane = next_id;
            if let Some(pane) = self.panes.get_mut(&next_id) {
                pane.editor.request_focus = true;
            }
        }
    }
}

impl eframe::App for MaterialTerminalApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        MaterialTheme::from_config(&self.config)
            .background
            .to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_background_events();
        self.handle_global_shortcuts(&ctx);

        let theme_key = (
            self.config.theme.clone(),
            self.config.window.opacity,
            self.config.font.size,
        );
        let theme = MaterialTheme::from_config(&self.config);
        if self.last_applied_font_family != self.config.font.family {
            apply_configured_font(&ctx, &self.config.font.family);
            self.last_applied_font_family = self.config.font.family.clone();
        }
        if self.last_applied_theme.as_ref() != Some(&theme_key) {
            theme.apply_to_egui(&ctx, &self.config);
            self.last_applied_theme = Some(theme_key);
        }

        // Request periodic repaint if any block is running (to animate live timer) or toast is active.
        let has_running = self.panes.values().any(|p| p.engine.has_running_block());
        if has_running || self.toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        let active_tab_idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        self.active_tab = active_tab_idx;
        let focused_pane_id = self
            .tabs
            .get(active_tab_idx)
            .map(|t| t.focused_pane)
            .unwrap_or(0);

        // Compute active search target block before rendering panes.
        let active_search_block = if self.search.open && !self.search.query.trim().is_empty() {
            let q = self.search.query.trim().to_ascii_lowercase();
            self.panes.get(&focused_pane_id).and_then(|p| {
                let ids: Vec<u64> = p
                    .engine
                    .blocks()
                    .iter()
                    .filter(|b| {
                        b.command_text.to_ascii_lowercase().contains(&q)
                            || b.plain_output_text().to_ascii_lowercase().contains(&q)
                    })
                    .map(|b| b.id)
                    .collect();
                ids.get(
                    self.search
                        .active_match_idx
                        .min(ids.len().saturating_sub(1)),
                )
                .copied()
            })
        } else {
            None
        };

        // Recover input focus automatically if user starts typing outside modals/alt-screen.
        if !self.palette.open && !self.search.open {
            let editor_id = Id::NULL.with(("material_input_editor", focused_pane_id));
            let editor_focused = ctx.memory(|m| m.has_focus(editor_id));
            let in_alt_screen = self
                .panes
                .get(&focused_pane_id)
                .is_some_and(|p| p.engine.is_alt_screen());
            if !editor_focused && !in_alt_screen {
                let typed_text: String = ctx.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Text(t) if !i.modifiers.command && !i.modifiers.ctrl => {
                                Some(t.as_str())
                            }
                            _ => None,
                        })
                        .collect()
                });
                if !typed_text.is_empty()
                    && let Some(pane) = self.panes.get_mut(&focused_pane_id)
                {
                    pane.editor.buffer.push_str(&typed_text);
                    pane.editor.request_focus = true;
                }
            }
        }

        CentralPanel::no_frame().frame(Frame::NONE).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            paint_liquid_backdrop(ui.painter(), ui.max_rect(), &theme);

            render_top_bar(ui, self, &theme);
            render_notifications(ui, self, &theme);
            render_search_bar(ui, self, &theme, focused_pane_id);

            let workspace_rect = ui.available_rect_before_wrap();
            if workspace_rect.width() > 1.0 && workspace_rect.height() > 1.0 {
                ui.allocate_rect(workspace_rect, egui::Sense::hover());
            }
            if let Some(mut root) = self.tabs.get(active_tab_idx).map(|t| t.root.clone()) {
                render_pane_tree(
                    ui,
                    &mut root,
                    workspace_rect,
                    self,
                    &theme,
                    focused_pane_id,
                    active_search_block,
                );
                if let Some(tab) = self.tabs.get_mut(active_tab_idx) {
                    tab.root = root;
                }
            }
        });

        // Floating Command Palette modal (`Cmd+K` / `Ctrl+Shift+P`)
        let active_theme = self.config.resolved_theme();
        if let Some(action) = self
            .palette
            .show(&ctx, &theme, active_theme, &self.recent_history)
        {
            self.apply_palette_action(action, &ctx);
        }
    }
}

fn parse_shortcut(spec: &str, default_mods: Modifiers, default_key: Key) -> (Modifiers, Key) {
    let mut mods = Modifiers::NONE;
    let mut parsed_key = None;
    for part in spec.split('+').map(|s| s.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "cmd" | "command" | "super" | "meta" => mods |= Modifiers::COMMAND,
            "ctrl" | "control" => mods |= Modifiers::CTRL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" | "opt" => mods |= Modifiers::ALT,
            "," | "comma" => parsed_key = Some(Key::Comma),
            "[" => parsed_key = Some(Key::OpenBracket),
            "]" => parsed_key = Some(Key::CloseBracket),
            other => {
                if let Some(k) = Key::from_name(&other.to_ascii_uppercase()) {
                    parsed_key = Some(k);
                }
            }
        }
    }
    match parsed_key {
        Some(k) if !mods.is_none() => (mods, k),
        _ => (default_mods, default_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_tree_splits_cycles_and_collapses_cleanly() {
        let mut root = PaneNode::Leaf(1);
        assert!(root.split_leaf(1, 2, SplitAxis::Vertical));
        assert!(root.split_leaf(2, 3, SplitAxis::Horizontal));
        assert_eq!(root.leaves(), vec![1, 2, 3]);

        assert_eq!(root.cycle_leaf(1, true), Some(2));
        assert_eq!(root.cycle_leaf(3, true), Some(1));
        assert_eq!(root.cycle_leaf(1, false), Some(3));

        let reduced = root.remove_leaf(2).expect("tree remains");
        assert_eq!(reduced.leaves(), vec![1, 3]);
        let single = reduced.remove_leaf(1).expect("one leaf remains");
        assert_eq!(single, PaneNode::Leaf(3));
        assert!(single.remove_leaf(3).is_none());
    }
}
