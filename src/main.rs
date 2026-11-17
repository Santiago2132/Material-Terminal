use material_terminal::config::ConfigManager;
use material_terminal::pty::{PtySession, PtySpawnConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (cfg_mgr, warning) = ConfigManager::load_or_create_default(None);
    if let Some(msg) = warning {
        eprintln!("Config warning: {msg}");
    }
    let cfg = cfg_mgr.current();
    let _pty = PtySession::spawn(
        PtySpawnConfig {
            program: cfg.shell.program.clone(),
            args: cfg.shell.args.clone(),
            enable_osc133_hooks: cfg.shell.enable_osc133_hooks,
            ..PtySpawnConfig::default()
        },
        None,
    )?;
    Ok(())
}
