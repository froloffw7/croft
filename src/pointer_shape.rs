//! The mouse pointer's shape, set through OSC 22.
//!
//! A terminal that speaks the sequence (kitty's pointer-shape protocol, and
//! any emulator that implements the same OSC 22) draws the named cursor over
//! its window. croft asks for an I-beam over text it edits (the editor, a
//! terminal pane, the Remote view) and the plain arrow everywhere else.
//!
//! The names are X11 cursor-font names (`xterm` is `XC_xterm`, `left_ptr` is
//! `XC_left_ptr`), which a terminal resolves through its cursor theme.
//! Support is asked for once at launch with `OSC 22 ; ?pointer ST`: a terminal
//! that has the feature answers `OSC 22 ; 1 ST`, one that knows the sequence
//! but not the shape `OSC 22 ; 0 ST`, and one that has never heard of it says
//! nothing. croft only sets a shape after a `1`.

/// What the pointer looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerShape {
    /// `XC_left_ptr`: the arrow, over chrome and lists.
    #[default]
    Arrow,
    /// `XC_xterm`: the I-beam, over text.
    Text,
}

impl PointerShape {
    /// The cursor-font name sent to the terminal.
    pub fn name(self) -> &'static str {
        match self {
            PointerShape::Arrow => "left_ptr",
            PointerShape::Text => "xterm",
        }
    }

    /// `OSC 22 ; <name> ST`, the request to draw this shape.
    pub fn set_sequence(self) -> Vec<u8> {
        format!("\x1b]22;{}\x1b\\", self.name()).into_bytes()
    }
}

/// `OSC 22 ; ?pointer ST`: ask whether the terminal has the `pointer` shape.
pub const QUERY_SEQUENCE: &[u8] = b"\x1b]22;?pointer\x1b\\";

/// True when `reply` holds the answer `OSC 22 ; 1 ST` (the terminal has the
/// shape). `OSC 22 ; 0 ST`, no answer, and bytes around it all read as no.
/// BEL ends the string as well as ST, as for every OSC.
pub fn reply_says_supported(reply: &[u8]) -> bool {
    const HEAD: &[u8] = b"\x1b]22;";
    reply
        .windows(HEAD.len())
        .enumerate()
        .filter(|(_, w)| *w == HEAD)
        .any(|(at, _)| {
            let tail = &reply[at + HEAD.len()..];
            tail.first() == Some(&b'1')
                && (tail[1..].starts_with(b"\x1b\\") || tail[1..].starts_with(b"\x07"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_is_asked_for_by_its_cursor_font_name() {
        assert_eq!(
            PointerShape::Arrow.set_sequence(),
            b"\x1b]22;left_ptr\x1b\\"
        );
        assert_eq!(PointerShape::Text.set_sequence(), b"\x1b]22;xterm\x1b\\");
    }

    #[test]
    fn only_a_one_means_the_terminal_has_the_shape() {
        assert!(reply_says_supported(b"\x1b]22;1\x1b\\"));
        assert!(reply_says_supported(b"\x1b]22;1\x07"));
        assert!(!reply_says_supported(b"\x1b]22;0\x1b\\"));
        assert!(!reply_says_supported(b""));
        // A bare `1` outside the OSC, or an unterminated one, is not an answer.
        assert!(!reply_says_supported(b"1"));
        assert!(!reply_says_supported(b"\x1b]22;1"));
    }

    #[test]
    fn the_answer_is_found_among_the_other_replies() {
        let reply = b"\x1b[?0u\x1b]22;1\x1b\\\x1b[?62;4c";
        assert!(reply_says_supported(reply));
        assert!(!reply_says_supported(b"\x1b[?0u\x1b]22;0\x1b\\\x1b[?62;4c"));
    }
}
