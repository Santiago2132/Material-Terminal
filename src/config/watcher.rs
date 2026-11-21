//! Fail-safe config loading, persistence, and live `notify` hot-reload watcher.

use super::schema::{AppConfig, default_config_path};
use super::themes::BuiltinTheme;
use crossbeam_channel::{Receiver, Sender, unbounded};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Events emitted by the hot-reload configuration watcher to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigEvent {
    Reloaded(Box<AppConfig>),
    ReloadError(String),
}

/// Manages config loading, saving, validation, and background file watching.
pub struct ConfigManager {
    path: PathBuf,
    current: Arc<Mutex<AppConfig>>,
    rx: Receiver<ConfigEvent>,
    _watcher: Option<RecommendedWatcher>,
}

impl ConfigManager {
    /// Initializes configuration from the default path and starts a live file watcher.
    pub fn load_or_create_default(
        wake_cb: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> (Self, Option<String>) {
        Self::load_or_create_at(default_config_path(), wake_cb)
    }

    /// Initializes configuration from a custom path (useful for isolated tests) and starts watching.
    pub fn load_or_create_at(
        path: PathBuf,
        wake_cb: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> (Self, Option<String>) {
        let mut startup_warning = None;
        let initial_config = match Self::ensure_and_load(&path) {
            Ok(cfg) => cfg,
            Err(err) => {
                startup_warning = Some(err);
                AppConfig::default()
            }
        };

        let current = Arc::new(Mutex::new(initial_config));
        let (tx, rx) = unbounded();
        let watcher = Self::spawn_watcher(&path, Arc::clone(&current), tx, wake_cb);

        (
            Self {
                path,
                current,
                rx,
                _watcher: watcher,
            },
            startup_warning,
        )
    }

    fn ensure_and_load(path: &Path) -> Result<AppConfig, String> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create config directory: {e}"))?;
            }
            let defaults = AppConfig::default();
            let raw = defaults
                .to_commented_toml()
                .map_err(|e| format!("Failed to serialize default config: {e}"))?;
            fs::write(path, raw)
                .map_err(|e| format!("Failed to write default config.toml: {e}"))?;
            return Ok(defaults);
        }

        Self::parse_and_validate_file(path)
    }

    fn parse_and_validate_file(path: &Path) -> Result<AppConfig, String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let parsed: AppConfig =
            toml::from_str(&raw).map_err(|e| format!("TOML syntax error in config.toml: {e}"))?;
        parsed.validate()?;
        Ok(parsed)
    }

    fn spawn_watcher(
        config_path: &Path,
        current: Arc<Mutex<AppConfig>>,
        tx: Sender<ConfigEvent>,
        wake_cb: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Option<RecommendedWatcher> {
        let watch_dir = config_path.parent()?.to_path_buf();
        let target_name = config_path.file_name()?.to_os_string();
        let config_path = config_path.to_path_buf();

        // Debounce burst file-system events from editors performing atomic write+rename.
        let last_reload = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else {
                return;
            };
            let relevant_kind = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
            );
            if !relevant_kind {
                return;
            }
            let touches_target = event.paths.is_empty()
                || event
                    .paths
                    .iter()
                    .any(|p| p.file_name() == Some(&target_name));
            if !touches_target {
                return;
            }

            {
                let mut guard = match last_reload.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                if guard.elapsed() < Duration::from_millis(60) {
                    return;
                }
                *guard = Instant::now();
            }

            match Self::parse_and_validate_file(&config_path) {
                Ok(new_cfg) => {
                    let mut guard = match current.lock() {
                        Ok(g) => g,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    if *guard != new_cfg {
                        *guard = new_cfg.clone();
                        let _ = tx.send(ConfigEvent::Reloaded(Box::new(new_cfg)));
                        if let Some(cb) = &wake_cb {
                            cb();
                        }
                    }
                }
                Err(err_msg) => {
                    let _ = tx.send(ConfigEvent::ReloadError(err_msg));
                    if let Some(cb) = &wake_cb {
                        cb();
                    }
                }
            }
        })
        .ok()?;

        watcher
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .ok()?;
        Some(watcher)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn current(&self) -> AppConfig {
        match self.current.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Non-blocking drain of pending hot-reload events.
    pub fn poll_events(&self) -> Vec<ConfigEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        events
    }

    /// Updates the theme in-memory and writes the updated config cleanly to disk.
    pub fn set_theme(&self, theme: BuiltinTheme) -> Result<AppConfig, String> {
        let mut cfg = self.current();
        cfg.theme = theme.id().to_string();
        cfg.validate()?;

        let serialized = cfg
            .to_commented_toml()
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
        }
        fs::write(&self.path, serialized)
            .map_err(|e| format!("Failed to write {}: {e}", self.path.display()))?;

        let mut guard = match self.current.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = cfg.clone();
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "material-terminal-test-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_default_config_and_validates_all_builtin_themes() {
        let dir = unique_temp_dir("defaults");
        let cfg_path = dir.join("config.toml");

        let (manager, warning) = ConfigManager::load_or_create_at(cfg_path.clone(), None);
        assert!(warning.is_none());
        assert!(cfg_path.exists());

        let cfg = manager.current();
        assert_eq!(cfg.resolved_theme(), BuiltinTheme::Obsidian);

        for theme in BuiltinTheme::ALL {
            let mut copy = cfg.clone();
            copy.theme = theme.display_name().to_string();
            assert!(copy.validate().is_ok());
            assert_eq!(copy.resolved_theme(), theme);
        }

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn validation_rejects_bad_ranges_and_unknown_theme() {
        let cfg = AppConfig {
            theme: "nonexistent-theme".to_string(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());

        let mut cfg = AppConfig::default();
        cfg.window.opacity = 1.5;
        assert!(cfg.validate().is_err());

        let mut cfg = AppConfig::default();
        cfg.font.size = 2.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn hot_reload_recovers_from_syntax_error_without_losing_last_valid() {
        let dir = unique_temp_dir("hotreload");
        let cfg_path = dir.join("config.toml");

        let (manager, warning) = ConfigManager::load_or_create_at(cfg_path.clone(), None);
        assert!(warning.is_none());
        assert_eq!(manager.current().resolved_theme(), BuiltinTheme::Obsidian);

        std::thread::sleep(Duration::from_millis(200));

        fs::write(&cfg_path, "theme = \"dracula\n[broken_toml").unwrap();

        let mut saw_error = false;
        for _ in 0..30 {
            for ev in manager.poll_events() {
                if matches!(ev, ConfigEvent::ReloadError(_)) {
                    saw_error = true;
                }
            }
            if saw_error {
                break;
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        assert!(saw_error, "expected ReloadError on broken TOML");
        assert_eq!(manager.current().resolved_theme(), BuiltinTheme::Obsidian);

        std::thread::sleep(Duration::from_millis(100));
        let mut updated = AppConfig {
            theme: "dracula".to_string(),
            ..Default::default()
        };
        updated.font.size = 15.0;
        fs::write(&cfg_path, updated.to_commented_toml().unwrap()).unwrap();

        let mut saw_reload = false;
        for _ in 0..30 {
            for ev in manager.poll_events() {
                if let ConfigEvent::Reloaded(new_cfg) = ev {
                    assert_eq!(new_cfg.resolved_theme(), BuiltinTheme::Dracula);
                    assert_eq!(new_cfg.font.size, 15.0);
                    saw_reload = true;
                }
            }
            if saw_reload {
                break;
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        assert!(saw_reload, "expected Reloaded event on valid config update");
        assert_eq!(manager.current().resolved_theme(), BuiltinTheme::Dracula);

        let _ = fs::remove_dir_all(dir);
    }
}
