# Material Terminal — Vision & Architectural Purpose

## Why Material Terminal?

Most modern terminals force a false choice: either ultra-minimal text grids built around 1970s teletype UX, or heavy Electron/cloud-tethered apps cluttered with telemetry, logins, and sidebars.

**Material Terminal** bridges two clear ideas in pure native Rust:

1. **Warp-Style Structured Command Execution:** Commands are not ephemeral lines scrolling off a flat buffer. Every execution is captured as an explicit **Command Block** with working directory, Git branch, ANSI-styled output, exit status, and duration—powered by standard `OSC 133` shell integration and `OSC 7` directory tracking.
2. **Material Design 3 Tonal Clarity:** Instead of neon chaos or flat chrome, the UI uses deliberate **surface hierarchy** (base canvas, elevated block cards, focused input dock, subtle hairline borders), readable monospace + UI typography, expressive color tokens (built-in *Tokyo Night*, *Catppuccin Mocha*, *Dracula*, and *Gruvbox Dark* themes), and responsive 120 FPS GPU rendering via `wgpu` (Metal / Vulkan).

---

## Core Design Principles

- **Zero Telemetry, Zero Accounts, Zero Bloat:** Everything runs locally. Command history lives in a local SQLite database (`~/.local/share/material-terminal/history.sqlite`), and configuration lives in a single human-readable TOML file (`~/.config/material-terminal/config.toml`).
- **Dual-Mode Terminal Engine:**
  - *Block Mode* for everyday commands: structured collapsible cards, copy/re-run actions, inline search, and a multiline bottom Input Editor with ghost-text completions.
  - *Alternate Screen Mode* for full-screen interactive TUI applications (`vim`, `nano`, `htop`, `less`): transparent handoff to a full terminal cell grid on `CSI ?1049h` and seamless return to blocks on `CSI ?1049l`.
- **Safe Customizability:** Hot-reload configuration with atomic fallback—syntax mistakes in `config.toml` surface an inline diagnostic banner without ever crashing active terminal sessions.
- **Native Multi-Pane Workspace:** Lightweight tabs and horizontal/vertical split panes, each owning an isolated PTY process and block timeline.

---

## Scope & Release Maturity (`v0.1.0-beta`)

This Beta release delivers a complete, daily-drivable foundation:

- **Included in `v0.1.0-beta`:**
  - Native GPU-accelerated window (`eframe` / `wgpu` / `winit`).
  - Asynchronous PTY engine (`portable-pty`) with non-invasive `zsh` & `bash` `OSC 133` + `OSC 7` shell hooks.
  - Full `vte` ANSI parser supporting TrueColor (24-bit RGB), 256-color, 16-color, SGR text styles, and Alternate Screen buffer grid.
  - Modern multiline Input Editor with mouse/keyboard cursor navigation, `Shift+Arrows` selection, `Option/Alt+Backspace` word deletion, SQLite + local-path ghost autocomplete, and history navigation.
  - Tabs, vertical/horizontal split panes, block actions (Copy Command, Copy Output, Re-run, Collapse/Expand), fuzzy Command Palette (`Cmd+K` / `Ctrl+Shift+P`), and inline search (`Cmd+F`).
  - Safe hot-reloading TOML configuration with 4 built-in themes.

- **Explicitly Deferred Post-Beta (Roadmap):**
  - GPU custom glyph ligature shaping & background blur shaders.
  - Fish/Nushell native shell integration scripts (currently runs via fallback block boundaries).
  - Session serialization/restore across restarts and plugin extensions.
