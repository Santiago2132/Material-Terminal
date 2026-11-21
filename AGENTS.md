# AGENTS.md — Material Terminal Architecture & Engineering Guidelines

## Project Overview

**Material Terminal** (`material-terminal`) is a native GPU-accelerated, Warp-inspired block terminal emulator written in Rust with a Material Design 3 surface hierarchy.

- **Repository:** `https://github.com/Santiago2132/Material-Terminal`
- **Primary Language:** Rust (Edition 2024 / `rustc >= 1.95`)
- **Rendering Stack:** `eframe` + `egui` backed by `wgpu` (Metal on macOS, Vulkan/DX12 elsewhere)

---

## Repository & Module Architecture

```text
src/
├── main.rs              # Application entrypoint, window/wgpu setup, event loop bootstrap
├── app.rs               # Top-level workspace state, tabs, notifications, shortcuts, Command Palette
├── config.rs            # AppConfig TOML schema, 4 built-in Material-adapted themes, notify hot-reload
├── history/
│   └── mod.rs           # Local SQLite persistence (rusqlite bundled) & path/history autocomplete engine
├── pty/
│   ├── mod.rs           # Async portable-pty spawn, resize, stdin writer, background reader thread
│   └── shell_hooks.rs   # Non-invasive zsh/bash OSC 133 (A/B/C/D) & OSC 7 wrapper generation
├── terminal/
│   └── mod.rs           # Alternate Screen Buffer cell grid & keyboard-to-VT byte encoder
├── blocks/
│   └── mod.rs           # CommandBlock model, vte::Perform ANSI/OSC 133 parser, async Git branch detector
└── ui/
    ├── mod.rs           # Workspace rendering, top tab bar, split-pane recursive layout, search & modals
    ├── block_view.rs    # Material-styled CommandBlock cards, ANSI spans, hover actions, collapse/expand
    ├── input_editor.rs  # Anchored multiline editor, selection, word deletion, ghost-text autocomplete
    ├── command_palette.rs # Fuzzy Command Palette (Cmd+K / Ctrl+Shift+P) & SQLite history search
    └── theme.rs         # Material Design 3 surface/elevation color derivation from active ThemePalette
```

---

## Key Architectural Invariants

1. **Never Block the UI Thread:**
   - PTY reads, `notify` file watching, and Git branch detection MUST run on background threads and communicate via non-blocking channels (`crossbeam_channel` / `std::sync::mpsc`) paired with `egui::Context::request_repaint()`.
2. **Fail-Safe Configuration:**
   - Invalid TOML edits in `~/.config/material-terminal/config.toml` MUST NOT crash or overwrite user files. Keep the last valid `AppConfig` and surface a non-intrusive diagnostic notification in the UI.
3. **OSC 133 + Fallback Robustness:**
   - Shell hooks emit `OSC 133;A` (prompt start), `OSC 133;B` (prompt end), `OSC 133;C` (command start), and `OSC 133;D;<code >` (command end).
   - Alternate screen (`CSI ?1049h` / `?1049l`) takes full pane viewport ownership for interactive TUIs (`vim`, `htop`, `less`) and cleanly restores block view upon exit.
4. **Zero Telemetry:**
   - Never add network telemetry, analytics, or external reporting. All persistence stays under local OS config/data directories.
5. **Simplicity & 400-Line File Limit:**
   - Every Rust source file (`src/**/*.rs`) MUST stay at or below **400 lines**. Whenever a file exceeds 400 lines, decompose it into cohesive, single-responsibility submodules so the codebase remains simple, readable, and easy to navigate.
6. **UI-Agnostic Core:**
   - Core modules (`config`, `pty`, `terminal`, `blocks`, `history`, `spaces`, `agents`) MUST NOT depend on any specific UI framework (`egui` / `gpui`). UI adapters live strictly in the UI layer.

---

## Git Branching & Commit Conventions

Strict Gitflow-lite discipline is enforced across all phases:

- **`main`**: Production/release-ready branch only. Never develop directly on `main`.
- **`development`**: Active integration branch. All feature branches branch off and merge back into `development`.
- **`feat/<topic>`**, **`fix/<topic>`**, **`docs/<topic>`**: Short-lived feature branches created from `development`.
- **Commits:** Follow [Conventional Commits](https://www.conventionalcommits.org/):
  - `feat(scope): ...`
  - `fix(scope): ...`
  - `chore(scope): ...`
  - `docs(scope): ...`
  - `test(scope): ...`
- **Verification Gate Before Merging:**
  - `cargo check --all-targets` passes cleanly.
  - `cargo test` passes with all unit and integration tests green.
  - `cargo clippy --all-targets` reports zero warnings on completion of phases.
