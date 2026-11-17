//! Safe configuration loading, validation, built-in Material-adapted themes,
//! and non-blocking live hot-reload via `notify`.

use crossbeam_channel::{Receiver, Sender, unbounded};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Application name used for `~/.config/<app>/config.toml` and state paths.
pub const APP_SLUG: &str = "material-terminal";

/// RGB triple for theme tokens and terminal colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Built-in color themes combining Liquid Glass tonal surfaces with classic developer palettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTheme {
    Obsidian,
    TokyoNight,
    TealFrost,
    CatppuccinMocha,
    Dracula,
    GruvboxDark,
}

impl BuiltinTheme {
    pub const ALL: [BuiltinTheme; 6] = [
        BuiltinTheme::Obsidian,
        BuiltinTheme::TokyoNight,
        BuiltinTheme::TealFrost,
        BuiltinTheme::CatppuccinMocha,
        BuiltinTheme::Dracula,
        BuiltinTheme::GruvboxDark,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Obsidian => "obsidian",
            Self::TokyoNight => "tokyo-night",
            Self::TealFrost => "teal-frost",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::Dracula => "dracula",
            Self::GruvboxDark => "gruvbox-dark",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Obsidian => "Obsidian",
            Self::TokyoNight => "Tokyo Night",
            Self::TealFrost => "Teal Frost",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::Dracula => "Dracula",
            Self::GruvboxDark => "Gruvbox Dark",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        match normalized.as_str() {
            "obsidian" | "liquid-glass" => Some(Self::Obsidian),
            "tokyo-night" | "tokyonight" | "tokyo" => Some(Self::TokyoNight),
            "teal-frost" | "frost" => Some(Self::TealFrost),
            "catppuccin-mocha" | "catppuccin" => Some(Self::CatppuccinMocha),
            "dracula" => Some(Self::Dracula),
            "gruvbox-dark" | "gruvbox" => Some(Self::GruvboxDark),
            _ => None,
        }
    }

    pub fn palette(self) -> ThemePalette {
        match self {
            Self::Obsidian => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(8, 12, 22),
                surface: Rgb::new(15, 20, 32),
                surface_elevated: Rgb::new(20, 27, 42),
                surface_highest: Rgb::new(30, 40, 61),
                orb_primary: Rgb::new(14, 92, 150),
                orb_secondary: Rgb::new(67, 56, 168),
                border: Rgb::new(42, 52, 74),
                border_focused: Rgb::new(56, 189, 248),
                foreground: Rgb::new(241, 245, 249),
                foreground_muted: Rgb::new(148, 163, 184),
                foreground_subtle: Rgb::new(100, 116, 139),
                accent: Rgb::new(56, 189, 248),
                accent_soft: Rgb::new(24, 58, 88),
                success: Rgb::new(52, 211, 153),
                error: Rgb::new(248, 113, 113),
                warning: Rgb::new(251, 191, 36),
                info: Rgb::new(56, 189, 248),
                selection_bg: Rgb::new(35, 68, 108),
                ansi: [
                    Rgb::new(15, 23, 42),
                    Rgb::new(248, 113, 113),
                    Rgb::new(52, 211, 153),
                    Rgb::new(251, 191, 36),
                    Rgb::new(56, 189, 248),
                    Rgb::new(167, 139, 250),
                    Rgb::new(34, 211, 238),
                    Rgb::new(203, 213, 225),
                    Rgb::new(71, 85, 105),
                    Rgb::new(252, 165, 165),
                    Rgb::new(110, 231, 183),
                    Rgb::new(253, 224, 71),
                    Rgb::new(125, 211, 252),
                    Rgb::new(196, 181, 253),
                    Rgb::new(103, 232, 249),
                    Rgb::new(248, 250, 252),
                ],
            },
            Self::TokyoNight => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(12, 15, 26),
                surface: Rgb::new(20, 22, 36),
                surface_elevated: Rgb::new(28, 31, 48),
                surface_highest: Rgb::new(41, 46, 68),
                orb_primary: Rgb::new(68, 28, 132),
                orb_secondary: Rgb::new(34, 74, 156),
                border: Rgb::new(48, 54, 78),
                border_focused: Rgb::new(122, 162, 247),
                foreground: Rgb::new(212, 220, 248),
                foreground_muted: Rgb::new(148, 158, 196),
                foreground_subtle: Rgb::new(92, 102, 144),
                accent: Rgb::new(122, 162, 247),
                accent_soft: Rgb::new(42, 59, 108),
                success: Rgb::new(158, 206, 106),
                error: Rgb::new(247, 118, 142),
                warning: Rgb::new(224, 175, 104),
                info: Rgb::new(125, 207, 255),
                selection_bg: Rgb::new(51, 70, 124),
                ansi: [
                    Rgb::new(21, 22, 30),
                    Rgb::new(247, 118, 142),
                    Rgb::new(158, 206, 106),
                    Rgb::new(224, 175, 104),
                    Rgb::new(122, 162, 247),
                    Rgb::new(187, 154, 247),
                    Rgb::new(125, 207, 255),
                    Rgb::new(169, 177, 214),
                    Rgb::new(65, 72, 104),
                    Rgb::new(255, 137, 157),
                    Rgb::new(185, 242, 124),
                    Rgb::new(255, 199, 119),
                    Rgb::new(139, 176, 255),
                    Rgb::new(202, 174, 255),
                    Rgb::new(164, 226, 255),
                    Rgb::new(213, 218, 246),
                ],
            },
            Self::TealFrost => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(9, 16, 24),
                surface: Rgb::new(16, 26, 36),
                surface_elevated: Rgb::new(22, 34, 46),
                surface_highest: Rgb::new(34, 49, 64),
                orb_primary: Rgb::new(15, 118, 110),
                orb_secondary: Rgb::new(29, 78, 126),
                border: Rgb::new(44, 64, 82),
                border_focused: Rgb::new(45, 212, 191),
                foreground: Rgb::new(240, 249, 250),
                foreground_muted: Rgb::new(148, 173, 184),
                foreground_subtle: Rgb::new(96, 122, 134),
                accent: Rgb::new(45, 212, 191),
                accent_soft: Rgb::new(22, 74, 76),
                success: Rgb::new(110, 231, 183),
                error: Rgb::new(253, 164, 175),
                warning: Rgb::new(252, 211, 77),
                info: Rgb::new(56, 189, 248),
                selection_bg: Rgb::new(28, 78, 86),
                ansi: [
                    Rgb::new(17, 24, 39),
                    Rgb::new(253, 164, 175),
                    Rgb::new(110, 231, 183),
                    Rgb::new(252, 211, 77),
                    Rgb::new(56, 189, 248),
                    Rgb::new(192, 132, 252),
                    Rgb::new(45, 212, 191),
                    Rgb::new(203, 213, 225),
                    Rgb::new(75, 85, 99),
                    Rgb::new(254, 205, 211),
                    Rgb::new(167, 243, 208),
                    Rgb::new(253, 230, 138),
                    Rgb::new(125, 211, 252),
                    Rgb::new(216, 180, 254),
                    Rgb::new(94, 234, 212),
                    Rgb::new(248, 250, 252),
                ],
            },
            Self::CatppuccinMocha => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(19, 19, 30),
                surface: Rgb::new(28, 28, 43),
                surface_elevated: Rgb::new(36, 36, 54),
                surface_highest: Rgb::new(49, 50, 68),
                orb_primary: Rgb::new(92, 62, 146),
                orb_secondary: Rgb::new(46, 86, 148),
                border: Rgb::new(66, 68, 88),
                border_focused: Rgb::new(137, 180, 250),
                foreground: Rgb::new(205, 214, 244),
                foreground_muted: Rgb::new(166, 173, 200),
                foreground_subtle: Rgb::new(108, 112, 134),
                accent: Rgb::new(137, 180, 250),
                accent_soft: Rgb::new(49, 64, 99),
                success: Rgb::new(166, 227, 161),
                error: Rgb::new(243, 139, 168),
                warning: Rgb::new(249, 226, 175),
                info: Rgb::new(137, 220, 235),
                selection_bg: Rgb::new(69, 71, 90),
                ansi: [
                    Rgb::new(69, 71, 90),
                    Rgb::new(243, 139, 168),
                    Rgb::new(166, 227, 161),
                    Rgb::new(249, 226, 175),
                    Rgb::new(137, 180, 250),
                    Rgb::new(245, 194, 231),
                    Rgb::new(148, 226, 213),
                    Rgb::new(186, 194, 222),
                    Rgb::new(88, 91, 112),
                    Rgb::new(243, 139, 168),
                    Rgb::new(166, 227, 161),
                    Rgb::new(249, 226, 175),
                    Rgb::new(137, 180, 250),
                    Rgb::new(245, 194, 231),
                    Rgb::new(148, 226, 213),
                    Rgb::new(205, 214, 244),
                ],
            },
            Self::Dracula => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(24, 25, 34),
                surface: Rgb::new(34, 36, 48),
                surface_elevated: Rgb::new(43, 46, 61),
                surface_highest: Rgb::new(62, 66, 86),
                orb_primary: Rgb::new(98, 56, 146),
                orb_secondary: Rgb::new(138, 48, 96),
                border: Rgb::new(72, 76, 98),
                border_focused: Rgb::new(189, 147, 249),
                foreground: Rgb::new(248, 248, 242),
                foreground_muted: Rgb::new(189, 194, 214),
                foreground_subtle: Rgb::new(98, 114, 164),
                accent: Rgb::new(189, 147, 249),
                accent_soft: Rgb::new(72, 56, 107),
                success: Rgb::new(80, 250, 123),
                error: Rgb::new(255, 85, 85),
                warning: Rgb::new(241, 250, 140),
                info: Rgb::new(139, 233, 253),
                selection_bg: Rgb::new(68, 71, 90),
                ansi: [
                    Rgb::new(33, 34, 44),
                    Rgb::new(255, 85, 85),
                    Rgb::new(80, 250, 123),
                    Rgb::new(241, 250, 140),
                    Rgb::new(189, 147, 249),
                    Rgb::new(255, 121, 198),
                    Rgb::new(139, 233, 253),
                    Rgb::new(248, 248, 242),
                    Rgb::new(98, 114, 164),
                    Rgb::new(255, 110, 110),
                    Rgb::new(105, 255, 148),
                    Rgb::new(255, 255, 165),
                    Rgb::new(214, 172, 255),
                    Rgb::new(255, 146, 223),
                    Rgb::new(164, 255, 255),
                    Rgb::new(255, 255, 255),
                ],
            },
            Self::GruvboxDark => ThemePalette {
                name: self.display_name(),
                id: self.id(),
                background: Rgb::new(24, 26, 27),
                surface: Rgb::new(36, 36, 36),
                surface_elevated: Rgb::new(46, 44, 43),
                surface_highest: Rgb::new(60, 56, 54),
                orb_primary: Rgb::new(112, 74, 24),
                orb_secondary: Rgb::new(56, 84, 72),
                border: Rgb::new(78, 72, 68),
                border_focused: Rgb::new(250, 189, 47),
                foreground: Rgb::new(235, 219, 178),
                foreground_muted: Rgb::new(189, 174, 147),
                foreground_subtle: Rgb::new(146, 131, 116),
                accent: Rgb::new(250, 189, 47),
                accent_soft: Rgb::new(87, 67, 30),
                success: Rgb::new(184, 187, 38),
                error: Rgb::new(251, 73, 52),
                warning: Rgb::new(254, 128, 25),
                info: Rgb::new(131, 165, 152),
                selection_bg: Rgb::new(80, 73, 69),
                ansi: [
                    Rgb::new(40, 40, 40),
                    Rgb::new(204, 36, 29),
                    Rgb::new(152, 151, 26),
                    Rgb::new(215, 153, 33),
                    Rgb::new(69, 133, 136),
                    Rgb::new(177, 98, 134),
                    Rgb::new(104, 157, 106),
                    Rgb::new(168, 153, 132),
                    Rgb::new(146, 131, 116),
                    Rgb::new(251, 73, 52),
                    Rgb::new(184, 187, 38),
                    Rgb::new(250, 189, 47),
                    Rgb::new(131, 165, 152),
                    Rgb::new(211, 134, 155),
                    Rgb::new(142, 192, 124),
                    Rgb::new(235, 219, 178),
                ],
            },
        }
    }
}

/// Complete theme color tokens derived for Liquid Glass surfaces and ANSI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub id: &'static str,
    pub name: &'static str,
    pub background: Rgb,
    pub surface: Rgb,
    pub surface_elevated: Rgb,
    pub surface_highest: Rgb,
    pub orb_primary: Rgb,
    pub orb_secondary: Rgb,
    pub border: Rgb,
    pub border_focused: Rgb,
    pub foreground: Rgb,
    pub foreground_muted: Rgb,
    pub foreground_subtle: Rgb,
    pub accent: Rgb,
    pub accent_soft: Rgb,
    pub success: Rgb,
    pub error: Rgb,
    pub warning: Rgb,
    pub info: Rgb,
    pub selection_bg: Rgb,
    pub ansi: [Rgb; 16],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono".to_string(),
            size: 13.5,
            line_height: 1.38,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub opacity: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub block_spacing: f32,
    pub corner_radius: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            padding_x: 18.0,
            padding_y: 12.0,
            block_spacing: 12.0,
            corner_radius: 14.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub enable_osc133_hooks: bool,
    pub max_blocks: usize,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            enable_osc133_hooks: true,
            max_blocks: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub new_tab: String,
    pub close_tab: String,
    pub split_vertical: String,
    pub split_horizontal: String,
    pub command_palette: String,
    pub search: String,
    pub clear_blocks: String,
    pub open_config: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_tab: "Cmd+T".to_string(),
            close_tab: "Cmd+W".to_string(),
            split_vertical: "Cmd+D".to_string(),
            split_horizontal: "Cmd+Shift+D".to_string(),
            command_palette: "Cmd+K".to_string(),
            search: "Cmd+F".to_string(),
            clear_blocks: "Cmd+L".to_string(),
            open_config: "Cmd+,".to_string(),
        }
    }
}

/// Top-level user configuration persisted as TOML in `~/.config/material-terminal/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: String,
    pub font: FontConfig,
    pub window: WindowConfig,
    pub shell: ShellConfig,
    pub keybindings: KeybindingsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: BuiltinTheme::Obsidian.id().to_string(),
            font: FontConfig::default(),
            window: WindowConfig::default(),
            shell: ShellConfig::default(),
            keybindings: KeybindingsConfig::default(),
        }
    }
}

impl AppConfig {
    /// Returns the resolved built-in theme, defaulting to Obsidian if invalid.
    pub fn resolved_theme(&self) -> BuiltinTheme {
        BuiltinTheme::from_name(&self.theme).unwrap_or(BuiltinTheme::Obsidian)
    }

    /// Returns the active Liquid Glass / Material color palette.
    pub fn palette(&self) -> ThemePalette {
        self.resolved_theme().palette()
    }

    /// Validates numeric ranges and theme selection.
    pub fn validate(&self) -> Result<(), String> {
        if BuiltinTheme::from_name(&self.theme).is_none() {
            return Err(format!(
                "Unknown theme '{}'. Valid themes: obsidian, tokyo-night, teal-frost, catppuccin-mocha, dracula, gruvbox-dark",
                self.theme
            ));
        }
        if !(8.0..=48.0).contains(&self.font.size) {
            return Err(format!(
                "font.size ({}) must be between 8.0 and 48.0",
                self.font.size
            ));
        }
        if !(1.0..=2.5).contains(&self.font.line_height) {
            return Err(format!(
                "font.line_height ({}) must be between 1.0 and 2.5",
                self.font.line_height
            ));
        }
        if self.font.family.trim().is_empty() {
            return Err("font.family cannot be empty".to_string());
        }
        if !(0.25..=1.0).contains(&self.window.opacity) {
            return Err(format!(
                "window.opacity ({}) must be between 0.25 and 1.0",
                self.window.opacity
            ));
        }
        if !(0.0..=64.0).contains(&self.window.padding_x)
            || !(0.0..=64.0).contains(&self.window.padding_y)
        {
            return Err("window padding values must be between 0.0 and 64.0".to_string());
        }
        if !(0.0..=32.0).contains(&self.window.block_spacing) {
            return Err("window.block_spacing must be between 0.0 and 32.0".to_string());
        }
        if !(0.0..=28.0).contains(&self.window.corner_radius) {
            return Err("window.corner_radius must be between 0.0 and 28.0".to_string());
        }
        if self.shell.max_blocks == 0 {
            return Err("shell.max_blocks must be greater than 0".to_string());
        }
        Ok(())
    }

    /// Generates an annotated default `config.toml` document.
    pub fn to_commented_toml(&self) -> Result<String, toml::ser::Error> {
        let body = toml::to_string_pretty(self)?;
        Ok(format!(
            "# Material Terminal Configuration\n\
             # Available themes: \"obsidian\", \"tokyo-night\", \"teal-frost\", \"catppuccin-mocha\", \"dracula\", \"gruvbox-dark\"\n\
             # Edits are hot-reloaded live. Invalid syntax preserves the previous working config.\n\n\
             {body}"
        ))
    }
}

/// Events emitted by the hot-reload configuration watcher to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigEvent {
    Reloaded(Box<AppConfig>),
    ReloadError(String),
}

/// Resolves the canonical config file path (`~/.config/material-terminal/config.toml`).
pub fn default_config_path() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("MATERIAL_TERMINAL_CONFIG_DIR") {
        return PathBuf::from(override_dir).join("config.toml");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join(APP_SLUG).join("config.toml");
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join(APP_SLUG).join("config.toml")
}

/// Resolves the canonical local data directory (`~/.local/share/material-terminal`).
pub fn default_data_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("MATERIAL_TERMINAL_DATA_DIR") {
        return PathBuf::from(override_dir);
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join(APP_SLUG);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".local").join("share").join(APP_SLUG)
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

        // Give macOS FSEvents a brief moment to register the directory watch.
        std::thread::sleep(Duration::from_millis(200));

        // 1. Write invalid TOML syntax -> should emit ReloadError and keep Obsidian.
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

        // 2. Write valid TOML -> should emit Reloaded with Dracula and update current().
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
