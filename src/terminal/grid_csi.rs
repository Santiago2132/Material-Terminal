//! CSI dispatch helpers for `AltScreenGrid`.

use super::grid::AltScreenGrid;
use super::style::apply_sgr_params;
use vte::Params;

impl AltScreenGrid {
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
                self.cursor_row = p(0, 1).saturating_sub(1).min(self.rows - 1);
                self.cursor_col = p(1, 1).saturating_sub(1).min(self.cols - 1);
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
            'J' => self.erase_display(p_zero(0)),
            'K' => self.erase_line(p_zero(0)),
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

    fn erase_display(&mut self, mode: usize) {
        let blank = self.blank_cell();
        match mode {
            0 => {
                let start = self.cursor_row * self.cols + self.cursor_col;
                self.cells[start..].fill(blank);
            }
            1 => {
                let end = (self.cursor_row * self.cols + self.cursor_col + 1).min(self.cells.len());
                self.cells[..end].fill(blank);
            }
            2 | 3 => self.cells.fill(blank),
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: usize) {
        let blank = self.blank_cell();
        let row_start = self.cursor_row * self.cols;
        match mode {
            0 => self.cells[(row_start + self.cursor_col)..(row_start + self.cols)].fill(blank),
            1 => {
                let to = (row_start + self.cursor_col + 1).min(row_start + self.cols);
                self.cells[row_start..to].fill(blank);
            }
            2 => self.cells[row_start..(row_start + self.cols)].fill(blank),
            _ => {}
        }
    }
}
