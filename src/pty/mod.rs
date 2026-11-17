//! Asynchronous PTY engine backed by `portable-pty` and non-blocking channels.

pub mod shell_hooks;

use crossbeam_channel::{Receiver, Sender, unbounded};
use portable_pty::{ChildKiller, CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use self::shell_hooks::{ShellFlavor, prepare_shell_launch, prepare_shell_launch_in};

/// Events emitted asynchronously from the background PTY reader/wait threads.
#[derive(Debug, Clone)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Exited(Option<i32>),
    Error(String),
}

enum PtyControl {
    Write(Vec<u8>),
    Resize(PtySize),
    Shutdown,
}

/// Configuration for spawning a PTY shell session.
#[derive(Debug, Clone)]
pub struct PtySpawnConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub enable_osc133_hooks: bool,
    pub extra_env: Vec<(String, String)>,
    pub custom_data_dir: Option<PathBuf>,
}

impl Default for PtySpawnConfig {
    fn default() -> Self {
        let working_dir = dirs::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            program: None,
            args: Vec::new(),
            working_dir,
            cols: 100,
            rows: 32,
            enable_osc133_hooks: true,
            extra_env: Vec::new(),
            custom_data_dir: None,
        }
    }
}

/// Resolves the user's preferred shell from `program` override, `$SHELL`, or OS defaults.
pub fn resolve_shell_program(override_program: Option<&str>) -> String {
    if let Some(prog) = override_program
        && !prog.trim().is_empty()
    {
        return prog.trim().to_string();
    }
    if let Ok(env_shell) = std::env::var("SHELL")
        && !env_shell.trim().is_empty()
    {
        return env_shell;
    }
    #[cfg(windows)]
    {
        if let Ok(comspec) = std::env::var("COMSPEC")
            && !comspec.trim().is_empty()
        {
            return comspec;
        }
        return "powershell.exe".to_string();
    }
    #[cfg(not(windows))]
    {
        if PathBuf::from("/bin/zsh").exists() {
            "/bin/zsh".to_string()
        } else if PathBuf::from("/bin/bash").exists() {
            "/bin/bash".to_string()
        } else {
            "/bin/sh".to_string()
        }
    }
}

/// Handle to a live PTY child shell running on dedicated background threads.
pub struct PtySession {
    control_tx: Sender<PtyControl>,
    event_rx: Receiver<PtyEvent>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    flavor: ShellFlavor,
    shell_program: String,
    last_size: PtySize,
}

impl PtySession {
    /// Spawns the configured shell in a fresh PTY without blocking the caller/UI thread.
    pub fn spawn(
        config: PtySpawnConfig,
        wake_cb: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, String> {
        let shell_program = resolve_shell_program(config.program.as_deref());
        let launch_plan = match &config.custom_data_dir {
            Some(dir) => prepare_shell_launch_in(
                &shell_program,
                &config.args,
                config.enable_osc133_hooks,
                dir,
            )?,
            None => prepare_shell_launch(&shell_program, &config.args, config.enable_osc133_hooks)?,
        };

        let pty_system = native_pty_system();
        let initial_size = PtySize {
            rows: config.rows.max(4),
            cols: config.cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(initial_size)
            .map_err(|e| format!("Failed to open PTY: {e}"))?;

        let mut cmd = CommandBuilder::new(&launch_plan.program);
        for arg in &launch_plan.args {
            cmd.arg(arg);
        }
        if config.working_dir.exists() {
            cmd.cwd(&config.working_dir);
        }
        for (k, v) in &launch_plan.env {
            cmd.env(k, v);
        }
        for (k, v) in &config.extra_env {
            cmd.env(k, v);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn shell '{}': {e}", launch_plan.program))?;
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {e}"))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY writer: {e}"))?;
        let master = pair.master;

        let (event_tx, event_rx) = unbounded::<PtyEvent>();
        let (control_tx, control_rx) = unbounded::<PtyControl>();

        // Reader thread: streams raw PTY output bytes immediately to UI channels.
        let reader_tx = event_tx.clone();
        let reader_wake = wake_cb.clone();
        thread::Builder::new()
            .name("pty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if reader_tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                                break;
                            }
                            if let Some(cb) = &reader_wake {
                                cb();
                            }
                        }
                        Err(err) => {
                            // EIO is normal on macOS/Linux when slave PTY closes on shell exit.
                            if err.raw_os_error() != Some(5) {
                                let _ = reader_tx
                                    .send(PtyEvent::Error(format!("PTY read error: {err}")));
                            }
                            break;
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn pty-reader thread: {e}"))?;

        // Writer + resize thread: owns master PTY handle so UI never blocks on write/ioctl.
        let writer_tx = event_tx.clone();
        thread::Builder::new()
            .name("pty-writer".to_string())
            .spawn(move || {
                while let Ok(msg) = control_rx.recv() {
                    match msg {
                        PtyControl::Write(bytes) => {
                            if let Err(err) = writer.write_all(&bytes) {
                                let _ = writer_tx
                                    .send(PtyEvent::Error(format!("PTY write error: {err}")));
                                break;
                            }
                            let _ = writer.flush();
                        }
                        PtyControl::Resize(size) => {
                            if let Err(err) = master.resize(size) {
                                let _ = writer_tx
                                    .send(PtyEvent::Error(format!("PTY resize error: {err}")));
                            }
                        }
                        PtyControl::Shutdown => break,
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn pty-writer thread: {e}"))?;

        // Child wait thread: notifies UI when the shell exits.
        let wait_wake = wake_cb;
        thread::Builder::new()
            .name("pty-wait".to_string())
            .spawn(move || {
                let code = child.wait().ok().map(|status| status.exit_code() as i32);
                let _ = event_tx.send(PtyEvent::Exited(code));
                if let Some(cb) = &wait_wake {
                    cb();
                }
            })
            .map_err(|e| format!("Failed to spawn pty-wait thread: {e}"))?;

        Ok(Self {
            control_tx,
            event_rx,
            killer,
            flavor: launch_plan.flavor,
            shell_program,
            last_size: initial_size,
        })
    }

    pub fn flavor(&self) -> ShellFlavor {
        self.flavor
    }

    pub fn shell_program(&self) -> &str {
        &self.shell_program
    }

    pub fn size(&self) -> PtySize {
        self.last_size
    }

    /// Queues bytes to be written to PTY stdin without blocking.
    pub fn write_bytes(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.control_tx.send(PtyControl::Write(bytes.to_vec()));
        }
    }

    /// Sends a command string to the shell, using bracketed paste when multiline.
    pub fn send_command(&self, command: &str) {
        if command.contains('\n') && matches!(self.flavor, ShellFlavor::Zsh | ShellFlavor::Bash) {
            let mut payload = Vec::with_capacity(command.len() + 16);
            payload.extend_from_slice(b"\x1b[200~");
            payload.extend_from_slice(command.as_bytes());
            payload.extend_from_slice(b"\x1b[201~\r");
            self.write_bytes(&payload);
        } else {
            let mut payload = Vec::with_capacity(command.len() + 1);
            payload.extend_from_slice(command.as_bytes());
            payload.push(b'\r');
            self.write_bytes(&payload);
        }
    }

    /// Sends Ctrl+C (`ETX`, `0x03`) to interrupt the foreground process group.
    pub fn send_interrupt(&self) {
        self.write_bytes(&[0x03]);
    }

    /// Sends a non-blocking PTY resize event if dimensions changed.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(10);
        let rows = rows.max(4);
        if cols == self.last_size.cols && rows == self.last_size.rows {
            return;
        }
        let next = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        self.last_size = next;
        let _ = self.control_tx.send(PtyControl::Resize(next));
    }

    /// Drains all currently buffered PTY events without blocking.
    pub fn drain_events(&self) -> Vec<PtyEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            events.push(ev);
        }
        events
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.control_tx.send(PtyControl::Shutdown);
        let _ = self.killer.kill();
    }
}

#[cfg(test)]
mod tests {
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

        assert!(
            collected.contains("\x1b]133;C"),
            "missing OSC 133;C in: {:?}",
            collected
        );
        assert!(
            collected.contains("MATERIAL_OK"),
            "missing command stdout in: {:?}",
            collected
        );
        assert!(
            collected.contains("\x1b]133;D;1"),
            "missing OSC 133;D;1 exit code in: {:?}",
            collected
        );

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
            "missing initial bash OSC 133;A/B in: {:?}",
            collected
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

        assert!(
            collected.contains("\x1b]133;C"),
            "missing bash OSC 133;C in: {:?}",
            collected
        );
        assert!(
            collected.contains("BASH_OK"),
            "missing bash stdout in: {:?}",
            collected
        );
        assert!(
            collected.contains("\x1b]133;D;0"),
            "missing bash OSC 133;D;0 in: {:?}",
            collected
        );

        drop(session);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
