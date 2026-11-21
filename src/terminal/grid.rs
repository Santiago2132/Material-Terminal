//! Full-viewport Alternate Screen Buffer cell grid (`CSI ?1049h/l`) with Unicode width support.

use super::style::TextStyle;
use unicode_width::UnicodeWidthChar;

/// Single character cell inside the Alternate Screen Buffer grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    pub ch: char,
    pub style: TextStyle,
    /// True for the trailing placeholder cell of a double-width (CJK/emoji) character.
    pub is_wide_continuation: bool,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: TextStyle::default(),
            is_wide_continuation: false,
        }
    }
}

/// Full-viewport terminal cell grid used when `CSI ?1049h` / `?1047h` / `?47h` is active.
#[derive(Debug, Clone)]
pub struct AltScreenGrid {
    pub(super) cols: usize,
    pub(super) rows: usize,
    pub(super) cells: Vec<GridCell>,
    pub(super) cursor_row: usize,
    pub(super) cursor_col: usize,
    pub(super) saved_cursor: (usize, usize, TextStyle),
    pub(super) scroll_top: usize,
    pub(super) scroll_bottom: usize,
    pub(super) wrap_pending: bool,
    pub(super) style: TextStyle,
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

    pub(super) fn blank_cell(&self) -> GridCell {
        GridCell {
            ch: ' ',
            style: TextStyle {
                bg: self.style.bg,
                ..TextStyle::default()
            },
            is_wide_continuation: false,
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
        let width = ch.width().unwrap_or(1);
        if width == 0 {
            return;
        }
        if self.wrap_pending || (width == 2 && self.cursor_col + 1 >= self.cols) {
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
                is_wide_continuation: false,
            },
        );
        if width == 2 && self.cursor_col + 1 < self.cols {
            self.set_cell(
                self.cursor_row,
                self.cursor_col + 1,
                GridCell {
                    ch: ' ',
                    style: self.style,
                    is_wide_continuation: true,
                },
            );
        }
        if self.cursor_col + width >= self.cols {
            self.wrap_pending = true;
        } else {
            self.cursor_col += width;
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

    pub(super) fn scroll_up_region(&mut self, count: usize) {
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

    pub(super) fn scroll_down_region(&mut self, count: usize) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::TermColor;
    use vte::{Params, Parser, Perform};

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
    fn alt_screen_handles_double_width_unicode_chars() {
        let mut grid = AltScreenGrid::new(20, 4);
        grid.print_char('漢');
        assert_eq!(grid.row_cells(0)[0].ch, '漢');
        assert!(!grid.row_cells(0)[0].is_wide_continuation);
        assert!(grid.row_cells(0)[1].is_wide_continuation);
        assert_eq!(grid.cursor(), (0, 2));
    }
}
