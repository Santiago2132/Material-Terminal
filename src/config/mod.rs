//! Safe configuration loading, validation, built-in Material-adapted themes,
//! and non-blocking live hot-reload via `notify`.

pub mod schema;
pub mod themes;
pub mod watcher;

pub use schema::{
    AppConfig, FontConfig, KeybindingsConfig, ShellConfig, WindowConfig, default_config_path,
    default_data_dir,
};
pub use themes::{BuiltinTheme, Rgb, ThemePalette};
pub use watcher::{ConfigEvent, ConfigManager};

/// Application name used for `~/.config/<app>/config.toml` and state paths.
pub const APP_SLUG: &str = "material-terminal";
