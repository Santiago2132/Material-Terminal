//! Adapter translating `egui::Key` / `egui::Modifiers` into UI-agnostic `TermKey` / `TermModifiers`.

use crate::terminal::{TermKey, TermModifiers, encode_term_key};
use egui::{Key, Modifiers};

/// Encodes an `egui` keyboard event into VT/ANSI terminal escape bytes for the PTY.
pub fn encode_egui_key(
    key: Key,
    modifiers: Modifiers,
    application_cursor_keys: bool,
) -> Option<Vec<u8>> {
    let term_key = map_egui_key(key)?;
    let term_mods = TermModifiers {
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
        #[cfg(target_os = "macos")]
        super_key: modifiers.mac_cmd,
        #[cfg(not(target_os = "macos"))]
        super_key: modifiers.command && !modifiers.ctrl,
    };
    encode_term_key(term_key, term_mods, application_cursor_keys)
}

fn map_egui_key(key: Key) -> Option<TermKey> {
    Some(match key {
        Key::Enter => TermKey::Enter,
        Key::Backspace => TermKey::Backspace,
        Key::Tab => TermKey::Tab,
        Key::Escape => TermKey::Escape,
        Key::ArrowUp => TermKey::ArrowUp,
        Key::ArrowDown => TermKey::ArrowDown,
        Key::ArrowLeft => TermKey::ArrowLeft,
        Key::ArrowRight => TermKey::ArrowRight,
        Key::Home => TermKey::Home,
        Key::End => TermKey::End,
        Key::PageUp => TermKey::PageUp,
        Key::PageDown => TermKey::PageDown,
        Key::Insert => TermKey::Insert,
        Key::Delete => TermKey::Delete,
        Key::F1 => TermKey::F1,
        Key::F2 => TermKey::F2,
        Key::F3 => TermKey::F3,
        Key::F4 => TermKey::F4,
        Key::F5 => TermKey::F5,
        Key::F6 => TermKey::F6,
        Key::F7 => TermKey::F7,
        Key::F8 => TermKey::F8,
        Key::F9 => TermKey::F9,
        Key::F10 => TermKey::F10,
        Key::F11 => TermKey::F11,
        Key::F12 => TermKey::F12,
        Key::A => TermKey::Char('a'),
        Key::B => TermKey::Char('b'),
        Key::C => TermKey::Char('c'),
        Key::D => TermKey::Char('d'),
        Key::E => TermKey::Char('e'),
        Key::F => TermKey::Char('f'),
        Key::G => TermKey::Char('g'),
        Key::H => TermKey::Char('h'),
        Key::I => TermKey::Char('i'),
        Key::J => TermKey::Char('j'),
        Key::K => TermKey::Char('k'),
        Key::L => TermKey::Char('l'),
        Key::M => TermKey::Char('m'),
        Key::N => TermKey::Char('n'),
        Key::O => TermKey::Char('o'),
        Key::P => TermKey::Char('p'),
        Key::Q => TermKey::Char('q'),
        Key::R => TermKey::Char('r'),
        Key::S => TermKey::Char('s'),
        Key::T => TermKey::Char('t'),
        Key::U => TermKey::Char('u'),
        Key::V => TermKey::Char('v'),
        Key::W => TermKey::Char('w'),
        Key::X => TermKey::Char('x'),
        Key::Y => TermKey::Char('y'),
        Key::Z => TermKey::Char('z'),
        Key::Space => TermKey::Char(' '),
        Key::OpenBracket => TermKey::Char('['),
        Key::Backslash => TermKey::Char('\\'),
        Key::CloseBracket => TermKey::Char(']'),
        _ => return None,
    })
}
