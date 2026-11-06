//! The mouse pointer's shape, set through OSC 22.
//!
//! A terminal that speaks the sequence (kitty's pointer-shape protocol, and
//! any emulator that implements the same OSC 22) draws the named cursor over
//! its window. croft asks for an I-beam over text it edits (the editor, a
//! terminal pane, the Remote view), a double arrow over the seams that resize
//! a split, and the plain arrow everywhere else.
//!
//! Two naming schemes exist for the shape and croft speaks both, chosen by the
//! `pointer_shape_names` setting ([`PointerShapeNames`]):
//!
//! - `x11` (the default): X11 cursor-font names (`xterm` is `XC_xterm`,
//!   `left_ptr` is `XC_left_ptr`), the names xterm's original proposal uses,
//!   which a terminal resolves through its cursor theme.
//! - `css`: the CSS `cursor` names (`text`, `default`, `ew-resize`,
//!   `ns-resize`) of kitty's pointer-shapes specification, whose names are
//!   limited to `a-z0-9_-` and do not include the X11 ones.
//!
//! References: <https://sw.kovidgoyal.net/kitty/pointer-shapes/>,
//! <https://vtdn.dev/docs/osc/osc22/>, <https://docs.otty.sh/vt/osc/osc-22>.
//!
//! Support is asked for once at launch with `OSC 22 ; ?pointer ST`. In
//! kitty's protocol a query answers `OSC 22 ; 1 ST` when the name is supported
//! and `OSC 22 ; 0 ST` when it is not; a terminal that has never heard of the
//! sequence says nothing. croft only sets a shape after a `1`.

use serde::{Deserialize, Serialize};

/// Which naming scheme the shape names are sent in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerShapeNames {
    /// X11 cursor-font names: `left_ptr`, `xterm`, `sb_h_double_arrow`,
    /// `sb_v_double_arrow`.
    #[default]
    X11,
    /// CSS cursor names (kitty's specification): `default`, `text`,
    /// `ew-resize`, `ns-resize`.
    Css,
}

/// What the pointer looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerShape {
    /// `XC_left_ptr`: the arrow, over chrome and lists.
    #[default]
    Arrow,
    /// `XC_xterm`: the I-beam, over text.
    Text,
    /// `XC_sb_h_double_arrow`: left and right, over a vertical seam.
    ResizeCols,
    /// `XC_sb_v_double_arrow`: up and down, over a horizontal seam.
    ResizeRows,
}

impl PointerShape {
    /// The name sent to the terminal, in the chosen scheme.
    pub fn name(self, names: PointerShapeNames) -> &'static str {
        match (self, names) {
            (PointerShape::Arrow, PointerShapeNames::X11) => "left_ptr",
            (PointerShape::Text, PointerShapeNames::X11) => "xterm",
            (PointerShape::ResizeCols, PointerShapeNames::X11) => "sb_h_double_arrow",
            (PointerShape::ResizeRows, PointerShapeNames::X11) => "sb_v_double_arrow",
            (PointerShape::Arrow, PointerShapeNames::Css) => "default",
            (PointerShape::Text, PointerShapeNames::Css) => "text",
            (PointerShape::ResizeCols, PointerShapeNames::Css) => "ew-resize",
            (PointerShape::ResizeRows, PointerShapeNames::Css) => "ns-resize",
        }
    }

    /// `OSC 22 ; <name> ST`, the request to draw this shape.
    pub fn set_sequence(self, names: PointerShapeNames) -> Vec<u8> {
        format!("\x1b]22;{}\x1b\\", self.name(names)).into_bytes()
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
    fn a_shape_is_asked_for_by_its_x11_cursor_font_name() {
        let x = PointerShapeNames::X11;
        assert_eq!(
            PointerShape::Arrow.set_sequence(x),
            b"\x1b]22;left_ptr\x1b\\"
        );
        assert_eq!(PointerShape::Text.set_sequence(x), b"\x1b]22;xterm\x1b\\");
        assert_eq!(
            PointerShape::ResizeCols.set_sequence(x),
            b"\x1b]22;sb_h_double_arrow\x1b\\"
        );
        assert_eq!(
            PointerShape::ResizeRows.set_sequence(x),
            b"\x1b]22;sb_v_double_arrow\x1b\\"
        );
    }

    #[test]
    fn the_css_names_follow_the_kitty_specification() {
        let c = PointerShapeNames::Css;
        assert_eq!(
            PointerShape::Arrow.set_sequence(c),
            b"\x1b]22;default\x1b\\"
        );
        assert_eq!(PointerShape::Text.set_sequence(c), b"\x1b]22;text\x1b\\");
        assert_eq!(
            PointerShape::ResizeCols.set_sequence(c),
            b"\x1b]22;ew-resize\x1b\\"
        );
        assert_eq!(
            PointerShape::ResizeRows.set_sequence(c),
            b"\x1b]22;ns-resize\x1b\\"
        );
        // The specification allows only a-z0-9_- in a name.
        for shape in [
            PointerShape::Arrow,
            PointerShape::Text,
            PointerShape::ResizeCols,
            PointerShape::ResizeRows,
        ] {
            assert!(
                shape.name(c).bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'_'
                    || b == b'-')
            );
        }
    }

    #[test]
    fn the_names_setting_parses_and_defaults_to_x11() {
        let c: PointerShapeNames = serde_json::from_str("\"css\"").unwrap();
        assert_eq!(c, PointerShapeNames::Css);
        assert_eq!(PointerShapeNames::default(), PointerShapeNames::X11);
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
