//! Shared ANSI SGR styling, Alternate Screen Buffer cell grid (`CSI ?1049h/l`),
//! and VT keyboard-to-bytes encoder for interactive TUI applications.

use crate::config::{Rgb, ThemePalette};
use egui::{Key, Modifiers};
use vte::Params;

/// Terminal color specification independent of active theme palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl TermColor {
    pub fn resolve_fg(self, palette: &ThemePalette) -> Rgb {
        match self {
            Self::Default => palette.foreground,
            Self::Indexed(idx) => indexed_color_to_rgb(idx, palette),
            Self::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }

    pub fn resolve_bg(self, palette: &ThemePalette, fallback: Rgb) -> Rgb {
        match self {
            Self::Default => fallback,
            Self::Indexed(idx) => indexed_color_to_rgb(idx, palette),
            Self::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }
}

/// Resolves an xterm 256-color index into concrete RGB using the active theme for 0..=15.
pub fn indexed_color_to_rgb(idx: u8, palette: &ThemePalette) -> Rgb {
    match idx {
        0..=15 => palette.ansi[idx as usize],
        16..=231 => {
            let n = idx - 16;
            let r = n / 36;
            let g = (n % 36) / 6;
            let b = n % 6;
            let map = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb::new(map(r), map(g), map(b))
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            Rgb::new(v, v, v)
        }
    }
}

/// Text attributes parsed from ANSI SGR (`ESC [...m`) sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl TextStyle {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Applies ANSI SGR parameters (supporting 16-color, 256-color, TrueColor semicolon & colon forms).
pub fn apply_sgr_params(style: &mut TextStyle, params: &Params) {
    if params.is_empty() {
        style.reset();
        return;
    }

    let mut iter = params.iter();
    while let Some(sub) = iter.next() {
        let code = sub.first().copied().unwrap_or(0);
        match code {
            0 => style.reset(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            7 => style.inverse = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            27 => style.inverse = false,
            30..=37 => style.fg = TermColor::Indexed((code - 30) as u8),
            38 => {
                if let Some(color) = parse_extended_color(sub, &mut iter) {
                    style.fg = color;
                }
            }
            39 => style.fg = TermColor::Default,
            40..=47 => style.bg = TermColor::Indexed((code - 40) as u8),
            48 => {
                if let Some(color) = parse_extended_color(sub, &mut iter) {
                    style.bg = color;
                }
            }
            49 => style.bg = TermColor::Default,
            90..=97 => style.fg = TermColor::Indexed((code - 90 + 8) as u8),
            100..=107 => style.bg = TermColor::Indexed((code - 100 + 8) as u8),
            _ => {}
        }
    }
}

fn parse_extended_color<'a, I>(sub: &[u16], iter: &mut I) -> Option<TermColor>
where
    I: Iterator<Item = &'a [u16]>,
{
    // Colon-delimited subparameters: e.g. `38:5:196` or `38:2::255:128:0` or `38:2:255:128:0`
    if sub.len() >= 2 {
        match sub[1] {
            5 if sub.len() >= 3 => {
                return Some(TermColor::Indexed(sub[2].min(255) as u8));
            }
            2 if sub.len() >= 6 => {
                return Some(TermColor::Rgb(
                    sub[3].min(255) as u8,
                    sub[4].min(255) as u8,
                    sub[5].min(255) as u8,
                ));
            }
            2 if sub.len() == 5 => {
                return Some(TermColor::Rgb(
                    sub[2].min(255) as u8,
                    sub[3].min(255) as u8,
                    sub[4].min(255) as u8,
                ));
            }
            _ => {}
        }
    }

    // Semicolon-delimited parameters: `38;5;n` or `38;2;r;g;b`
    let mode = iter.next()?.first().copied()?;
    match mode {
        5 => {
            let idx = iter.next()?.first().copied()?;
            Some(TermColor::Indexed(idx.min(255) as u8))
        }
        2 => {
            let r = iter.next()?.first().copied()?.min(255) as u8;
            let g = iter.next()?.first().copied()?.min(255) as u8;
            let b = iter.next()?.first().copied()?.min(255) as u8;
            Some(TermColor::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Single character cell inside the Alternate Screen Buffer grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    pub ch: char,
    pub style: TextStyle,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: TextStyle::default(),
        }
    }
}

/// Full-viewport terminal cell grid used when `CSI ?1049h` / `?1047h` / `?47h` is active.
#[derive(Debug, Clone)]
pub struct AltScreenGrid {
    cols: usize,
    rows: usize,
    cells: Vec<GridCell>,
    cursor_row: usize,
    cursor_col: usize,
    saved_cursor: (usize, usize, TextStyle),
    scroll_top: usize,
    scroll_bottom: usize,
    wrap_pending: bool,
    style: TextStyle,
    pub cursor_visible: bool,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
}

impl AltScreenGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(10);
        let rows = rows.max(4);
        Self {
            cols,
            rows,
            cells: vec![GridCell::default(); cols * rows],
            cursor_row: 0,
            cursor_col: 0,
            saved_cursor: (0, 0, TextStyle::default()),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            wrap_pending: false,
            style: TextStyle::default(),
            cursor_visible: true,
            application_cursor_keys: false,
            bracketed_paste: false,
        }
    }

    pub fn reset(&mut self, cols: usize, rows: usize) {
        *self = Self::new(cols, rows);
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn row_cells(&self, row: usize) -> &[GridCell] {
        let start = row * self.cols;
        &self.cells[start..start + self.cols]
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.max(10);
        let new_rows = new_rows.max(4);
        if new_cols == self.cols && new_rows == self.rows {
            return;
        }
        let mut next = vec![GridCell::default(); new_cols * new_rows];
        let copy_rows = self.rows.min(new_rows);
        let copy_cols = self.cols.min(new_cols);
        for r in 0..copy_rows {
            for c in 0..copy_cols {
                next[r * new_cols + c] = self.cells[r * self.cols + c];
            }
        }
        self.cells = next;
        self.cols = new_cols;
        self.rows = new_rows;
        self.cursor_row = self.cursor_row.min(new_rows - 1);
        self.cursor_col = self.cursor_col.min(new_cols - 1);
        self.scroll_top = 0;
        self.scroll_bottom = new_rows - 1;
        self.wrap_pending = false;
    }

    fn blank_cell(&self) -> GridCell {
        GridCell {
            ch: ' ',
            style: TextStyle {
                bg: self.style.bg,
                ..TextStyle::default()
            },
        }
    }

    fn set_cell(&mut self, row: usize, col: usize, cell: GridCell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
        }
    }

    pub fn print_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if self.wrap_pending {
            self.wrap_pending = false;
            self.cursor_col = 0;
            self.line_feed();
        }
        self.set_cell(
            self.cursor_row,
            self.cursor_col,
            GridCell {
                ch,
                style: self.style,
            },
        );
        if self.cursor_col + 1 >= self.cols {
            self.wrap_pending = true;
        } else {
            self.cursor_col += 1;
        }
    }

    pub fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => {
                self.cursor_col = 0;
                self.wrap_pending = false;
            }
            b'\n' | 0x0b | 0x0c => {
                self.wrap_pending = false;
                self.line_feed();
            }
            0x08 => {
                self.wrap_pending = false;
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            b'\t' => {
                self.wrap_pending = false;
                let next_tab = ((self.cursor_col / 8) + 1) * 8;
                self.cursor_col = next_tab.min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up_region(1);
        } else if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor_row == self.scroll_top {
            self.scroll_down_region(1);
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
    }

    fn scroll_up_region(&mut self, count: usize) {
        let top = self.scroll_top.min(self.rows - 1);
        let bottom = self.scroll_bottom.min(self.rows - 1);
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let shift = count.min(height);
        for r in top..=(bottom - shift) {
            let src = (r + shift) * self.cols;
            let dst = r * self.cols;
            self.cells.copy_within(src..src + self.cols, dst);
        }
        let blank = self.blank_cell();
        for r in (bottom + 1 - shift)..=bottom {
            let start = r * self.cols;
            self.cells[start..start + self.cols].fill(blank);
        }
    }

    fn scroll_down_region(&mut self, count: usize) {
        let top = self.scroll_top.min(self.rows - 1);
        let bottom = self.scroll_bottom.min(self.rows - 1);
        if top > bottom {
            return;
        }
        let height = bottom - top + 1;
        let shift = count.min(height);
        for r in ((top + shift)..=bottom).rev() {
            let src = (r - shift) * self.cols;
            let dst = r * self.cols;
            self.cells.copy_within(src..src + self.cols, dst);
        }
        let blank = self.blank_cell();
        for r in top..(top + shift) {
            let start = r * self.cols;
            self.cells[start..start + self.cols].fill(blank);
        }
    }

    pub fn esc_dispatch(&mut self, intermediates: &[u8], byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'7' => self.saved_cursor = (self.cursor_row, self.cursor_col, self.style),
            b'8' => {
                self.cursor_row = self.saved_cursor.0.min(self.rows - 1);
                self.cursor_col = self.saved_cursor.1.min(self.cols - 1);
                self.style = self.saved_cursor.2;
                self.wrap_pending = false;
            }
            b'D' => {
                self.wrap_pending = false;
                self.line_feed();
            }
            b'E' => {
                self.wrap_pending = false;
                self.cursor_col = 0;
                self.line_feed();
            }
            b'M' => self.reverse_index(),
            b'c' => self.reset(self.cols, self.rows),
            _ => {}
        }
    }

    pub fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], action: char) {
        let p = |idx: usize, default: usize| -> usize {
            params
                .iter()
                .nth(idx)
                .and_then(|s| s.first().copied())
                .map(|v| if v == 0 { default } else { v as usize })
                .unwrap_or(default)
        };
        let p_zero = |idx: usize| -> usize {
            params
                .iter()
                .nth(idx)
                .and_then(|s| s.first().copied())
                .unwrap_or(0) as usize
        };

        if intermediates == *b"?" {
            let set = action == 'h';
            if action == 'h' || action == 'l' {
                for sub in params.iter() {
                    match sub.first().copied().unwrap_or(0) {
                        1 => self.application_cursor_keys = set,
                        25 => self.cursor_visible = set,
                        2004 => self.bracketed_paste = set,
                        _ => {}
                    }
                }
            }
            return;
        }

        if !intermediates.is_empty() {
            return;
        }

        match action {
            'H' | 'f' => {
                let row = p(0, 1).saturating_sub(1).min(self.rows - 1);
                let col = p(1, 1).saturating_sub(1).min(self.cols - 1);
                self.cursor_row = row;
                self.cursor_col = col;
                self.wrap_pending = false;
            }
            'A' => {
                self.cursor_row = self.cursor_row.saturating_sub(p(0, 1)).max(self.scroll_top);
                self.wrap_pending = false;
            }
            'B' => {
                self.cursor_row = (self.cursor_row + p(0, 1)).min(self.scroll_bottom);
                self.wrap_pending = false;
            }
            'C' => {
                self.cursor_col = (self.cursor_col + p(0, 1)).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'D' => {
                self.cursor_col = self.cursor_col.saturating_sub(p(0, 1));
                self.wrap_pending = false;
            }
            'G' => {
                self.cursor_col = p(0, 1).saturating_sub(1).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'd' => {
                self.cursor_row = p(0, 1).saturating_sub(1).min(self.rows - 1);
                self.wrap_pending = false;
            }
            'J' => {
                let blank = self.blank_cell();
                match p_zero(0) {
                    0 => {
                        let start = self.cursor_row * self.cols + self.cursor_col;
                        self.cells[start..].fill(blank);
                    }
                    1 => {
                        let end = (self.cursor_row * self.cols + self.cursor_col + 1)
                            .min(self.cells.len());
                        self.cells[..end].fill(blank);
                    }
                    2 | 3 => {
                        self.cells.fill(blank);
                    }
                    _ => {}
                }
            }
            'K' => {
                let blank = self.blank_cell();
                let row_start = self.cursor_row * self.cols;
                match p_zero(0) {
                    0 => {
                        let from = row_start + self.cursor_col;
                        let to = row_start + self.cols;
                        self.cells[from..to].fill(blank);
                    }
                    1 => {
                        let to = (row_start + self.cursor_col + 1).min(row_start + self.cols);
                        self.cells[row_start..to].fill(blank);
                    }
                    2 => {
                        self.cells[row_start..row_start + self.cols].fill(blank);
                    }
                    _ => {}
                }
            }
            'L' => {
                if self.cursor_row >= self.scroll_top && self.cursor_row <= self.scroll_bottom {
                    let saved_top = self.scroll_top;
                    self.scroll_top = self.cursor_row;
                    self.scroll_down_region(p(0, 1));
                    self.scroll_top = saved_top;
                }
            }
            'M' => {
                if self.cursor_row >= self.scroll_top && self.cursor_row <= self.scroll_bottom {
                    let saved_top = self.scroll_top;
                    self.scroll_top = self.cursor_row;
                    self.scroll_up_region(p(0, 1));
                    self.scroll_top = saved_top;
                }
            }
            'P' => {
                let count = p(0, 1).min(self.cols - self.cursor_col);
                let row_start = self.cursor_row * self.cols;
                let dst = row_start + self.cursor_col;
                let src_end = row_start + self.cols;
                if dst + count < src_end {
                    self.cells.copy_within((dst + count)..src_end, dst);
                }
                let blank = self.blank_cell();
                self.cells[(src_end - count)..src_end].fill(blank);
            }
            '@' => {
                let count = p(0, 1).min(self.cols - self.cursor_col);
                let row_start = self.cursor_row * self.cols;
                let dst = row_start + self.cursor_col;
                let row_end = row_start + self.cols;
                if dst + count < row_end {
                    self.cells.copy_within(dst..(row_end - count), dst + count);
                }
                let blank = self.blank_cell();
                self.cells[dst..(dst + count)].fill(blank);
            }
            'X' => {
                let count = p(0, 1).min(self.cols - self.cursor_col);
                let start = self.cursor_row * self.cols + self.cursor_col;
                let blank = self.blank_cell();
                self.cells[start..(start + count)].fill(blank);
            }
            'S' => self.scroll_up_region(p(0, 1)),
            'T' => self.scroll_down_region(p(0, 1)),
            'r' => {
                let top = p(0, 1).saturating_sub(1).min(self.rows - 1);
                let bottom = p(1, self.rows).saturating_sub(1).min(self.rows - 1);
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                } else {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows - 1;
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.wrap_pending = false;
            }
            's' => self.saved_cursor = (self.cursor_row, self.cursor_col, self.style),
            'u' => {
                self.cursor_row = self.saved_cursor.0.min(self.rows - 1);
                self.cursor_col = self.saved_cursor.1.min(self.cols - 1);
                self.style = self.saved_cursor.2;
                self.wrap_pending = false;
            }
            'm' => apply_sgr_params(&mut self.style, params),
            _ => {}
        }
    }
}

/// Encodes an egui keyboard event into VT/ANSI terminal escape bytes for the PTY.
pub fn encode_egui_key(
    key: Key,
    modifiers: Modifiers,
    application_cursor_keys: bool,
) -> Option<Vec<u8>> {
    // Handle Ctrl+<letter> (e.g., Ctrl+C -> 0x03, Ctrl+D -> 0x04, Ctrl+L -> 0x0c)
    if modifiers.ctrl
        && !modifiers.command
        && !modifiers.alt
        && let Some(ctrl_byte) = key_to_ctrl_byte(key)
    {
        return Some(vec![ctrl_byte]);
    }
    #[cfg(target_os = "macos")]
    if modifiers.ctrl
        && !modifiers.mac_cmd
        && !modifiers.alt
        && let Some(ctrl_byte) = key_to_ctrl_byte(key)
    {
        return Some(vec![ctrl_byte]);
    }

    let csi_prefix = if application_cursor_keys {
        "\x1bO"
    } else {
        "\x1b["
    };

    let seq: Option<&[u8]> = match key {
        Key::Enter => Some(b"\r"),
        Key::Backspace => Some(b"\x7f"),
        Key::Tab => Some(if modifiers.shift { b"\x1b[Z" } else { b"\t" }),
        Key::Escape => Some(b"\x1b"),
        Key::ArrowUp => {
            return Some(format!("{csi_prefix}A").into_bytes());
        }
        Key::ArrowDown => {
            return Some(format!("{csi_prefix}B").into_bytes());
        }
        Key::ArrowRight => {
            return Some(format!("{csi_prefix}C").into_bytes());
        }
        Key::ArrowLeft => {
            return Some(format!("{csi_prefix}D").into_bytes());
        }
        Key::Home => Some(if application_cursor_keys {
            b"\x1bOH"
        } else {
            b"\x1b[H"
        }),
        Key::End => Some(if application_cursor_keys {
            b"\x1bOF"
        } else {
            b"\x1b[F"
        }),
        Key::PageUp => Some(b"\x1b[5~"),
        Key::PageDown => Some(b"\x1b[6~"),
        Key::Insert => Some(b"\x1b[2~"),
        Key::Delete => Some(b"\x1b[3~"),
        Key::F1 => Some(b"\x1bOP"),
        Key::F2 => Some(b"\x1bOQ"),
        Key::F3 => Some(b"\x1bOR"),
        Key::F4 => Some(b"\x1bOS"),
        Key::F5 => Some(b"\x1b[15~"),
        Key::F6 => Some(b"\x1b[17~"),
        Key::F7 => Some(b"\x1b[18~"),
        Key::F8 => Some(b"\x1b[19~"),
        Key::F9 => Some(b"\x1b[20~"),
        Key::F10 => Some(b"\x1b[21~"),
        Key::F11 => Some(b"\x1b[23~"),
        Key::F12 => Some(b"\x1b[24~"),
        _ => None,
    };

    seq.map(|s| s.to_vec())
}

fn key_to_ctrl_byte(key: Key) -> Option<u8> {
    let idx = match key {
        Key::A => 1,
        Key::B => 2,
        Key::C => 3,
        Key::D => 4,
        Key::E => 5,
        Key::F => 6,
        Key::G => 7,
        Key::H => 8,
        Key::I => 9,
        Key::J => 10,
        Key::K => 11,
        Key::L => 12,
        Key::M => 13,
        Key::N => 14,
        Key::O => 15,
        Key::P => 16,
        Key::Q => 17,
        Key::R => 18,
        Key::S => 19,
        Key::T => 20,
        Key::U => 21,
        Key::V => 22,
        Key::W => 23,
        Key::X => 24,
        Key::Y => 25,
        Key::Z => 26,
        Key::Space => 0,
        Key::OpenBracket => 27,
        Key::Backslash => 28,
        Key::CloseBracket => 29,
        _ => return None,
    };
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vte::{Parser, Perform};

    struct GridAdapter<'a>(&'a mut AltScreenGrid);
    impl Perform for GridAdapter<'_> {
        fn print(&mut self, c: char) {
            self.0.print_char(c);
        }
        fn execute(&mut self, byte: u8) {
            self.0.execute(byte);
        }
        fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _: bool, action: char) {
            self.0.csi_dispatch(params, intermediates, action);
        }
        fn esc_dispatch(&mut self, intermediates: &[u8], _: bool, byte: u8) {
            self.0.esc_dispatch(intermediates, byte);
        }
    }

    #[test]
    fn alt_screen_positions_cursor_erases_and_parses_truecolor() {
        let mut grid = AltScreenGrid::new(40, 12);
        let mut parser = Parser::new();
        let mut adapter = GridAdapter(&mut grid);
        parser.advance(
            &mut adapter,
            b"\x1b[2;5H\x1b[38;2;255;120;40m\x1b[1mVIM\x1b[0m",
        );

        assert_eq!(grid.row_cells(1)[4].ch, 'V');
        assert_eq!(grid.row_cells(1)[4].style.fg, TermColor::Rgb(255, 120, 40));
        assert!(grid.row_cells(1)[4].style.bold);
        assert_eq!(grid.row_cells(1)[6].ch, 'M');
        assert_eq!(grid.cursor(), (1, 7));
    }

    #[test]
    fn encodes_decckm_and_ctrl_c() {
        assert_eq!(
            encode_egui_key(Key::ArrowUp, Modifiers::NONE, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_egui_key(Key::ArrowUp, Modifiers::NONE, false),
            Some(b"\x1b[A".to_vec())
        );
        let ctrl_c = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(encode_egui_key(Key::C, ctrl_c, false), Some(vec![0x03]));
    }
}
