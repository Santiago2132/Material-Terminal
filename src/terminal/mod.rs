//! Shared ANSI SGR styling, Alternate Screen Buffer cell grid (`CSI ?1049h/l`),
//! and UI-agnostic VT keyboard-to-bytes encoder for interactive TUI applications.

pub mod grid;
mod grid_csi;
pub mod input;
pub mod style;

pub use grid::{AltScreenGrid, GridCell};
pub use input::{TermKey, TermModifiers, encode_term_key};
pub use style::{TermColor, TextStyle, apply_sgr_params, indexed_color_to_rgb};
