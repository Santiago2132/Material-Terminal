//! TOML configuration schema, validation, and canonical OS path resolution.

use super::APP_SLUG;
use super::themes::{BuiltinTheme, ThemePalette};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        #[cfg(target_os = "macos")]
        {
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
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                new_tab: "Ctrl+Shift+T".to_string(),
                close_tab: "Ctrl+Shift+W".to_string(),
                split_vertical: "Ctrl+Shift+D".to_string(),
                split_horizontal: "Ctrl+Shift+E".to_string(),
                command_palette: "Ctrl+Shift+P".to_string(),
                search: "Ctrl+Shift+F".to_string(),
                clear_blocks: "Ctrl+Shift+L".to_string(),
                open_config: "Ctrl+,".to_string(),
            }
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
