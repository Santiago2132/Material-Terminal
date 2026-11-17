//! Non-invasive shell integration scripts that emit OSC 133 (A/B/C/D) semantic
//! prompt/command lifecycle markers and OSC 7 working-directory updates without
//! altering the user's `~/.zshrc` or `~/.bashrc`.

use crate::config::default_data_dir;
use std::fs;
use std::path::{Path, PathBuf};

/// Supported shell flavors for automatic OSC 133 / OSC 7 injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFlavor {
    Zsh,
    Bash,
    Other,
}

impl ShellFlavor {
    pub fn detect(shell_program: &str) -> Self {
        let name = Path::new(shell_program)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(shell_program)
            .to_ascii_lowercase();
        if name.contains("zsh") {
            Self::Zsh
        } else if name.contains("bash") {
            Self::Bash
        } else {
            Self::Other
        }
    }
}

/// Prepared shell invocation adjustments (arguments & environment overrides).
#[derive(Debug, Clone)]
pub struct ShellLaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub flavor: ShellFlavor,
}

/// Builds a launch plan and writes wrapper hook scripts under `data_dir/shell/`.
pub fn prepare_shell_launch(
    shell_program: &str,
    user_args: &[String],
    enable_hooks: bool,
) -> Result<ShellLaunchPlan, String> {
    prepare_shell_launch_in(shell_program, user_args, enable_hooks, &default_data_dir())
}

/// Same as [`prepare_shell_launch`], parameterized by state directory for isolated tests.
pub fn prepare_shell_launch_in(
    shell_program: &str,
    user_args: &[String],
    enable_hooks: bool,
    data_dir: &Path,
) -> Result<ShellLaunchPlan, String> {
    let flavor = ShellFlavor::detect(shell_program);
    let mut env = vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "Material-Terminal".to_string()),
        (
            "TERM_PROGRAM_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
    ];

    if !enable_hooks {
        return Ok(ShellLaunchPlan {
            program: shell_program.to_string(),
            args: user_args.to_vec(),
            env,
            flavor,
        });
    }

    match flavor {
        ShellFlavor::Zsh => {
            let zdotdir = write_zsh_wrapper(data_dir)?;
            let orig_zdotdir = std::env::var("ZDOTDIR")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| dirs::home_dir().map(|h| h.to_string_lossy().into_owned()))
                .unwrap_or_else(|| ".".to_string());

            env.push(("MATERIAL_ORIG_ZDOTDIR".to_string(), orig_zdotdir));
            env.push((
                "MATERIAL_WRAPPER_ZDOTDIR".to_string(),
                zdotdir.to_string_lossy().into_owned(),
            ));
            env.push(("ZDOTDIR".to_string(), zdotdir.to_string_lossy().into_owned()));

            let args = if user_args.is_empty() {
                vec!["-l".to_string(), "-i".to_string()]
            } else {
                user_args.to_vec()
            };

            Ok(ShellLaunchPlan {
                program: shell_program.to_string(),
                args,
                env,
                flavor,
            })
        }
        ShellFlavor::Bash => {
            let rcfile = write_bash_wrapper(data_dir)?;
            let mut args = if user_args.is_empty() {
                vec![
                    "--rcfile".to_string(),
                    rcfile.to_string_lossy().into_owned(),
                    "-i".to_string(),
                ]
            } else {
                user_args.to_vec()
            };
            // If user supplied custom flags without --rcfile, prepend --rcfile if safe.
            if !user_args.is_empty() && !user_args.iter().any(|a| a == "--rcfile") {
                args.insert(0, rcfile.to_string_lossy().into_owned());
                args.insert(0, "--rcfile".to_string());
            }
            Ok(ShellLaunchPlan {
                program: shell_program.to_string(),
                args,
                env,
                flavor,
            })
        }
        ShellFlavor::Other => Ok(ShellLaunchPlan {
            program: shell_program.to_string(),
            args: user_args.to_vec(),
            env,
            flavor,
        }),
    }
}

fn write_zsh_wrapper(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("shell").join("zsh");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create zsh hook directory {}: {e}", dir.display()))?;

    let zshenv = r#"# Material Terminal non-invasive .zshenv shim
export ZDOTDIR="${MATERIAL_ORIG_ZDOTDIR:-$HOME}"
if [[ -z "${MATERIAL_SKIP_USER_RC:-}" && -f "$ZDOTDIR/.zshenv" ]]; then
  source "$ZDOTDIR/.zshenv"
fi
export MATERIAL_USER_ZDOTDIR="$ZDOTDIR"
if [[ -o interactive ]]; then
  export ZDOTDIR="${MATERIAL_WRAPPER_ZDOTDIR}"
fi
"#;

    let zprofile = r#"# Material Terminal non-invasive .zprofile shim
export ZDOTDIR="${MATERIAL_USER_ZDOTDIR:-$HOME}"
if [[ -z "${MATERIAL_SKIP_USER_RC:-}" && -f "$ZDOTDIR/.zprofile" ]]; then
  source "$ZDOTDIR/.zprofile"
fi
if [[ -o interactive ]]; then
  export ZDOTDIR="${MATERIAL_WRAPPER_ZDOTDIR}"
fi
"#;

    let zshrc = r#"# Material Terminal non-invasive .zshrc shim (OSC 133 + OSC 7)
export ZDOTDIR="${MATERIAL_USER_ZDOTDIR:-$HOME}"
if [[ -z "${MATERIAL_SKIP_USER_RC:-}" && -f "$ZDOTDIR/.zshrc" ]]; then
  source "$ZDOTDIR/.zshrc"
fi

if [[ -z "${__MATERIAL_SHELL_HOOKS_LOADED:-}" ]]; then
  __MATERIAL_SHELL_HOOKS_LOADED=1
  __material_cmd_active=0
  unsetopt PROMPT_SP 2>/dev/null || PROMPT_EOL_MARK=''

  autoload -Uz add-zsh-hook 2>/dev/null || true

  __material_osc7() {
    local pwd_url="${PWD// /%20}"
    printf '\033]7;file://%s%s\007' "${HOST:-localhost}" "$pwd_url"
  }

  __material_precmd() {
    local exit_code=$?
    if [[ "$__material_cmd_active" -eq 1 ]]; then
      printf '\033]133;D;%d\007' "$exit_code"
      __material_cmd_active=0
    fi
    __material_osc7
    printf '\033]133;A\007'
    if [[ "$PS1" != *"133;B"* ]]; then
      PS1="${PS1}"$'%{\033]133;B\007%}'
    fi
  }

  __material_preexec() {
    __material_cmd_active=1
    printf '\033]133;C\007'
  }

  if typeset -f add-zsh-hook >/dev/null 2>&1; then
    add-zsh-hook precmd __material_precmd
    add-zsh-hook preexec __material_preexec
  else
    precmd_functions+=(__material_precmd)
    preexec_functions+=(__material_preexec)
  fi
fi

if [[ -o login ]]; then
  export ZDOTDIR="${MATERIAL_WRAPPER_ZDOTDIR}"
else
  export ZDOTDIR="${MATERIAL_USER_ZDOTDIR:-$HOME}"
  unset MATERIAL_WRAPPER_ZDOTDIR MATERIAL_ORIG_ZDOTDIR MATERIAL_USER_ZDOTDIR
fi
"#;

    let zlogin = r#"# Material Terminal non-invasive .zlogin shim
export ZDOTDIR="${MATERIAL_USER_ZDOTDIR:-$HOME}"
if [[ -z "${MATERIAL_SKIP_USER_RC:-}" && -f "$ZDOTDIR/.zlogin" ]]; then
  source "$ZDOTDIR/.zlogin"
fi
unset MATERIAL_WRAPPER_ZDOTDIR MATERIAL_ORIG_ZDOTDIR MATERIAL_USER_ZDOTDIR
"#;

    fs::write(dir.join(".zshenv"), zshenv).map_err(|e| format!("write .zshenv: {e}"))?;
    fs::write(dir.join(".zprofile"), zprofile).map_err(|e| format!("write .zprofile: {e}"))?;
    fs::write(dir.join(".zshrc"), zshrc).map_err(|e| format!("write .zshrc: {e}"))?;
    fs::write(dir.join(".zlogin"), zlogin).map_err(|e| format!("write .zlogin: {e}"))?;

    Ok(dir)
}

fn write_bash_wrapper(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("shell").join("bash");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create bash hook directory {}: {e}", dir.display()))?;

    let bashrc = r#"# Material Terminal non-invasive bash integration (OSC 133 + OSC 7)
if [[ -z "${MATERIAL_SKIP_USER_RC:-}" ]]; then
  [[ -f /etc/profile ]] && source /etc/profile
  if [[ -f "$HOME/.bash_profile" ]]; then
    source "$HOME/.bash_profile"
  elif [[ -f "$HOME/.bash_login" ]]; then
    source "$HOME/.bash_login"
  elif [[ -f "$HOME/.profile" ]]; then
    source "$HOME/.profile"
  fi
  [[ -f "$HOME/.bashrc" ]] && source "$HOME/.bashrc"
fi

if [[ -z "${__MATERIAL_SHELL_HOOKS_LOADED:-}" ]]; then
  __MATERIAL_SHELL_HOOKS_LOADED=1
  __material_in_prompt=1
  __material_cmd_active=0

  __material_preexec() {
    [[ -n "${COMP_LINE:-}" ]] && return
    [[ "$__material_in_prompt" -eq 1 ]] && return
    if [[ "$__material_cmd_active" -eq 0 ]]; then
      __material_cmd_active=1
      printf '\033]133;C\007'
    fi
  }

  __material_precmd() {
    local exit_code=$?
    __material_in_prompt=1
    if [[ "$__material_cmd_active" -eq 1 ]]; then
      printf '\033]133;D;%d\007' "$exit_code"
      __material_cmd_active=0
    fi
    local pwd_url="${PWD// /%20}"
    printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$pwd_url"
    printf '\033]133;A\007'
    if [[ "$PS1" != *"133;B"* ]]; then
      PS1="${PS1}\[\033]133;B\007\]"
    fi
    __material_in_prompt=0
  }

  trap '__material_preexec' DEBUG
  if [[ -n "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="__material_precmd; ${PROMPT_COMMAND}"
  else
    PROMPT_COMMAND="__material_precmd"
  fi
fi
"#;

    let path = dir.join("bashrc");
    fs::write(&path, bashrc).map_err(|e| format!("write bashrc: {e}"))?;
    Ok(path)
}
