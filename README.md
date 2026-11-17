# Material Terminal (`v0.1.0-beta`)

A native GPU-accelerated, Warp-inspired block terminal emulator written in Rust with **Material Design 3** surface clarity, zero bloat, and zero telemetry.

Built with `eframe` + `egui` over `wgpu` (Metal on macOS, Vulkan/DX12 on Linux/Windows), `portable-pty`, `vte`, `notify`, `rusqlite`, and `fuzzy-matcher`.

---

## Core Features

- **Structured Command Blocks (`OSC 133` + `OSC 7`):**
  - Non-invasive `zsh` and `bash` shell integration injected at session launch without touching `~/.zshrc` or `~/.bashrc`.
  - Each command runs inside an elevated Material card displaying working directory, asynchronous Git branch badge, ANSI TrueColor / 256-color / SGR styled output, execution duration pill, and status accent bar (`Running`, `Success`, `Error`).
  - Hover actions on every block: **Copy Command**, **Copy Output**, **Re-run**, and **Collapse / Expand**.
- **Seamless Alternate Screen Buffer (`CSI ?1049h` / `?1049l`):**
  - Full-pane terminal cell grid with keyboard/VT forwarding for interactive TUI applications (`vim`, `nano`, `htop`, `less`), restoring block timeline automatically on exit.
- **Modern Anchored Multiline Input Editor:**
  - Bottom-docked editor supporting multiline composition (`Shift+Enter`), mouse cursor placement, `Shift + Arrows` selection, `Option/Alt + Backspace` word deletion, navigable command history (`Up`/`Down`), and **inline ghost-text autocomplete** (`Tab` / `Right`) backed by local SQLite history and filesystem path matching.
  - Automatic **process stdin forwarding** while a foreground command is running, plus one-key interrupt (`Ctrl+C`) and EOF (`Ctrl+D`).
- **Workspace Tabs, Split Panes & Fuzzy Command Palette:**
  - Minimalist top tab strip with multi-tab support and recursive draggable split panes (`Cmd+D` vertical split, `Cmd+Shift+D` horizontal split), each owning an independent PTY session.
  - Fuzzy **Command Palette** (`Cmd+K` / `Ctrl+Shift+P`) to switch themes live, split panes, clear blocks, open `config.toml`, or search & re-run SQLite command history.
  - Inline **Block Search** (`Cmd+F`) with live match highlighting and Next/Prev navigation.
- **Fail-Safe Hot-Reload Configuration (`config.toml`):**
  - Real-time file watching via `notify`. Invalid TOML edits never crash active sessions—the last valid configuration stays active while surfacing a diagnostic banner.
  - 4 built-in Material-adapted themes: **Tokyo Night**, **Catppuccin Mocha**, **Dracula**, and **Gruvbox Dark**.

---

## Architecture

```text
src/
├── main.rs                 # Native wgpu/eframe window bootstrap
├── app.rs                  # Workspace tabs, split-pane binary tree, shortcuts, event polling
├── config.rs               # AppConfig TOML schema, validation, 4 themes, notify hot-reload
├── history/
│   └── mod.rs              # Local SQLite persistence (rusqlite bundled) & ghost autocomplete
├── pty/
│   ├── mod.rs              # Non-blocking portable-pty reader/writer/resize threads
│   └── shell_hooks.rs      # Non-invasive zsh & bash OSC 133 (A/B/C/D) + OSC 7 wrappers
├── terminal/
│   └── mod.rs              # SGR parser, Alternate Screen cell grid, VT key encoder
├── blocks/
│   └── mod.rs              # CommandBlock model, vte::Perform parser, async Git detector
└── ui/
    ├── mod.rs              # Tab bar, recursive split layout, search bar, Alt Screen renderer
    ├── block_view.rs       # Material CommandBlock cards & ANSI LayoutJob highlighting
    ├── input_editor.rs     # Anchored multiline editor, selection, history & ghost text
    ├── command_palette.rs  # Fuzzy Command Palette modal (SkimMatcherV2)
    └── theme.rs            # Material Design 3 surface tokens & system font loader
```

For architectural invariants and contribution guidelines, see [VISION.md](VISION.md) and [AGENTS.md](AGENTS.md).

---

## Customization (`~/.config/material-terminal/config.toml`)

On first launch, Material Terminal generates `~/.config/material-terminal/config.toml` automatically. Edits take effect immediately on save:

```toml
theme = "tokyo-night" # "tokyo-night" | "catppuccin-mocha" | "dracula" | "gruvbox-dark"

[font]
family = "JetBrains Mono"
size = 13.5
line_height = 1.38

[window]
opacity = 0.96
padding_x = 14.0
padding_y = 10.0
block_spacing = 10.0
corner_radius = 12.0

[shell]
# program = "/bin/zsh"
args = []
enable_osc133_hooks = true
max_blocks = 500

[keybindings]
new_tab = "Cmd+T"
close_tab = "Cmd+W"
split_vertical = "Cmd+D"
split_horizontal = "Cmd+Shift+D"
command_palette = "Cmd+K"
search = "Cmd+F"
clear_blocks = "Cmd+L"
open_config = "Cmd+,"
```

---

## Keyboard Shortcuts

| Shortcut | Action |
| --- | --- |
| `Cmd+K` / `Ctrl+Shift+P` | Open Command Palette |
| `Cmd+T` | New Tab |
| `Cmd+W` | Close Active Pane / Tab |
| `Cmd+D` | Split Pane Vertically (Side by Side) |
| `Cmd+Shift+D` | Split Pane Horizontally (Top / Bottom) |
| `Cmd+]` / `Cmd+[` | Cycle Focus Across Split Panes |
| `Cmd+1` .. `Cmd+9` | Jump to Tab |
| `Cmd+F` | Find in Command Blocks |
| `Cmd+L` | Clear Completed Blocks |
| `Cmd+,` | Open `config.toml` |
| `Enter` / `Shift+Enter` | Execute Command / Insert Newline |
| `Tab` / `Right Arrow` | Accept Ghost Autocomplete |
| `Up` / `Down` | Navigate Command History |
| `Option/Alt + Backspace` | Delete Previous Word |
| `Ctrl+C` / `Ctrl+D` | Interrupt / EOF Running Process |

---

## Build & Run

### Prerequisites

- **Rust toolchain** (`rustc >= 1.95`, Edition 2024)
- **macOS:** Xcode Command Line Tools (`xcode-select --install`) — uses Metal via `wgpu`
- **Linux:** `pkg-config`, `libxkbcommon-dev`, `libwayland-dev`, `libx11-dev`, `libfontconfig1-dev` (see `.github/workflows/ci.yml`)

### Commands

```bash
# Clone repository
git clone https://github.com/Santiago2132/Material-Terminal.git
cd Material-Terminal

# Run test suite & strict linter
cargo test
cargo clippy --all-targets -- -D warnings

# Build optimized release binary
cargo build --release

# Launch Material Terminal
./target/release/material-terminal
```
