//! UI-agnostic keyboard representation and VT/ANSI escape sequence encoder.

/// UI-independent keyboard modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

/// UI-independent key variants used to encode terminal escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKey {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

/// Encodes a UI-agnostic keyboard event into VT/ANSI terminal escape bytes for the PTY.
pub fn encode_term_key(
    key: TermKey,
    modifiers: TermModifiers,
    application_cursor_keys: bool,
) -> Option<Vec<u8>> {
    if modifiers.ctrl
        && !modifiers.super_key
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
        TermKey::Enter => Some(b"\r"),
        TermKey::Backspace => Some(b"\x7f"),
        TermKey::Tab => Some(if modifiers.shift { b"\x1b[Z" } else { b"\t" }),
        TermKey::Escape => Some(b"\x1b"),
        TermKey::ArrowUp => return Some(format!("{csi_prefix}A").into_bytes()),
        TermKey::ArrowDown => return Some(format!("{csi_prefix}B").into_bytes()),
        TermKey::ArrowRight => return Some(format!("{csi_prefix}C").into_bytes()),
        TermKey::ArrowLeft => return Some(format!("{csi_prefix}D").into_bytes()),
        TermKey::Home => Some(if application_cursor_keys {
            b"\x1bOH"
        } else {
            b"\x1b[H"
        }),
        TermKey::End => Some(if application_cursor_keys {
            b"\x1bOF"
        } else {
            b"\x1b[F"
        }),
        TermKey::PageUp => Some(b"\x1b[5~"),
        TermKey::PageDown => Some(b"\x1b[6~"),
        TermKey::Insert => Some(b"\x1b[2~"),
        TermKey::Delete => Some(b"\x1b[3~"),
        TermKey::F1 => Some(b"\x1bOP"),
        TermKey::F2 => Some(b"\x1bOQ"),
        TermKey::F3 => Some(b"\x1bOR"),
        TermKey::F4 => Some(b"\x1bOS"),
        TermKey::F5 => Some(b"\x1b[15~"),
        TermKey::F6 => Some(b"\x1b[17~"),
        TermKey::F7 => Some(b"\x1b[18~"),
        TermKey::F8 => Some(b"\x1b[19~"),
        TermKey::F9 => Some(b"\x1b[20~"),
        TermKey::F10 => Some(b"\x1b[21~"),
        TermKey::F11 => Some(b"\x1b[23~"),
        TermKey::F12 => Some(b"\x1b[24~"),
        TermKey::Char(_) => None,
    };

    seq.map(|s| s.to_vec())
}

fn key_to_ctrl_byte(key: TermKey) -> Option<u8> {
    let TermKey::Char(ch) = key else {
        return None;
    };
    match ch.to_ascii_lowercase() {
        'a'..='z' => Some((ch.to_ascii_lowercase() as u8) - b'a' + 1),
        ' ' => Some(0),
        '[' => Some(27),
        '\\' => Some(28),
        ']' => Some(29),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_decckm_and_ctrl_c() {
        assert_eq!(
            encode_term_key(TermKey::ArrowUp, TermModifiers::default(), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_term_key(TermKey::ArrowUp, TermModifiers::default(), false),
            Some(b"\x1b[A".to_vec())
        );
        let ctrl = TermModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            encode_term_key(TermKey::Char('c'), ctrl, false),
            Some(vec![0x03])
        );
    }
}
