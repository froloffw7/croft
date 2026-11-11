//! Shared hover-feedback helpers.
//!
//! Clickable surfaces across the side panels (file tree, remote hosts,
//! source-control changes, search results) and the panel-header action pills
//! all want the same affordance: the row or element under the pointer lifts to
//! a subtle background, mirroring VS Code's `list.hoverBackground` /
//! `toolbar.hoverBackground`. Rather than re-derive the hit-test at every call
//! site, each renderer feeds its render-time pointer cell into a
//! `hover_pointer` field (set by the app from `pointer_cell`, exactly like
//! `EditorTabs`) and asks these helpers what to paint.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicU8, Ordering};

/// VS Code `list.hoverBackground`, fitted to each croft theme. Deliberately
/// subtler than any selection fill so a hovered-but-unselected row never reads
/// as selected. Picked from a true-color mockup: a neutral-navy lift on Croft
/// Dark, a plain dark-grey lift on the OLED-black theme.
const ROW_HOVER_BG_DARK: Color = Color::Rgb(0x2b, 0x31, 0x42);
const ROW_HOVER_BG_BLACK: Color = Color::Rgb(0x1a, 0x1a, 0x1a);
const ROW_HOVER_BG_LIGHT: Color = Color::Rgb(0xf2, 0xf2, 0xf2);

/// True when `pointer` rests on any cell of `rect`. The single hit-test every
/// hover affordance is built on.
pub fn contains(rect: Rect, pointer: Option<(u16, u16)>) -> bool {
    pointer.is_some_and(|(px, py)| {
        px >= rect.x
            && px < rect.x.saturating_add(rect.width)
            && py >= rect.y
            && py < rect.y.saturating_add(rect.height)
    })
}

/// The row-hover background for the active theme, or `None` when the pointer
/// is not over `rect`. Callers paint this only on rows that are not already
/// selected/marked, so it never masks a stronger state. VS Code's light
/// `list.hoverBackground` (#f2f2f2) on light themes; the gradient brand
/// (Croft Black) keeps its grey lift, every other dark theme the navy one.
pub fn row_hover_bg(
    rect: Rect,
    pointer: Option<(u16, u16)>,
    theme: crate::theme::Theme,
) -> Option<Color> {
    contains(rect, pointer).then(|| hover_bg(theme))
}

/// The hover lift itself, for a hovered element that is not a whole row,
/// such as a link in the Markdown preview (#1939).
pub fn hover_bg(theme: crate::theme::Theme) -> Color {
    if theme.is_light() {
        ROW_HOVER_BG_LIGHT
    } else if theme.gradient() {
        ROW_HOVER_BG_BLACK
    } else {
        ROW_HOVER_BG_DARK
    }
}

/// How a hovered *heading* (a panel tab such as PROBLEMS, a section title)
/// shows the pointer: the `hover_heading` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadingHover {
    /// The text takes its hover colour (the default).
    #[default]
    Color,
    /// The hover colour, and bold.
    Bold,
    /// The hover colour on a pill of the toolbar hover background.
    Pill,
}

impl HeadingHover {
    /// Parse the setting; anything unknown (or an older config's empty
    /// string) is the default.
    pub fn from_pref(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "bold" => Self::Bold,
            "pill" => Self::Pill,
            _ => Self::Color,
        }
    }
}

/// How a hovered *icon* (header pill glyphs, the open-tabs list button, the
/// layout toggles, `+` / `-` / `⋯`) shows the pointer: the `hover_icon`
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconHover {
    /// The glyph takes the theme's accent (the default).
    #[default]
    Accent,
    /// The glyph brightens to white, in no hue of its own.
    Color,
    /// The glyph keeps its colour and gets a pill of the toolbar hover
    /// background.
    Pill,
}

impl IconHover {
    /// Parse the setting; anything unknown is the default.
    pub fn from_pref(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "color" => Self::Color,
            "pill" => Self::Pill,
            _ => Self::Accent,
        }
    }
}

static HEADING_MODE: AtomicU8 = AtomicU8::new(0);
static ICON_MODE: AtomicU8 = AtomicU8::new(0);

/// Apply the `hover_heading` and `hover_icon` settings. A process-wide switch,
/// like the Tab Size, so the hundreds of draw sites need no extra argument.
pub fn set_modes(heading: HeadingHover, icon: IconHover) {
    HEADING_MODE.store(heading as u8, Ordering::Relaxed);
    ICON_MODE.store(icon as u8, Ordering::Relaxed);
}

fn heading_mode() -> HeadingHover {
    match HEADING_MODE.load(Ordering::Relaxed) {
        1 => HeadingHover::Bold,
        2 => HeadingHover::Pill,
        _ => HeadingHover::Color,
    }
}

fn icon_mode() -> IconHover {
    match ICON_MODE.load(Ordering::Relaxed) {
        1 => IconHover::Color,
        2 => IconHover::Pill,
        _ => IconHover::Accent,
    }
}

/// The colour an interactive icon takes under the pointer: the active
/// theme's accent, so every theme highlights in its own hue.
pub fn icon_hover_fg(theme: crate::theme::Theme) -> Color {
    theme.accent()
}

/// The pill under a hovered heading or icon (VS Code
/// `toolbar.hoverBackground`), from the active theme.
pub fn pill_bg(theme: crate::theme::Theme) -> Color {
    theme.tab_close_pill_bg()
}

/// `rest` for a heading as drawn while hovered per `mode`; `hover_fg` is the
/// heading's brighter text colour.
pub fn heading_style_in(
    mode: HeadingHover,
    theme: crate::theme::Theme,
    rest: Style,
    hover_fg: Color,
    hovered: bool,
) -> Style {
    if !hovered {
        return rest;
    }
    let lit = rest.fg(hover_fg);
    match mode {
        HeadingHover::Color => lit,
        HeadingHover::Bold => lit.add_modifier(Modifier::BOLD),
        HeadingHover::Pill => lit.bg(pill_bg(theme)),
    }
}

/// [`heading_style_in`] under the `hover_heading` setting.
pub fn heading_style(
    theme: crate::theme::Theme,
    rest: Style,
    hover_fg: Color,
    hovered: bool,
) -> Style {
    heading_style_in(heading_mode(), theme, rest, hover_fg, hovered)
}

/// `rest` for an icon as drawn while hovered per `mode`.
pub fn icon_style_in(
    mode: IconHover,
    theme: crate::theme::Theme,
    rest: Style,
    hovered: bool,
) -> Style {
    if !hovered {
        return rest;
    }
    match mode {
        IconHover::Accent => rest.fg(icon_hover_fg(theme)),
        IconHover::Color => rest.fg(theme.ui(Color::White)),
        IconHover::Pill => rest.bg(pill_bg(theme)),
    }
}

/// Paint the pill behind a hovered icon across `area` (the icon's whole click
/// target, so the fill is as wide as the icon is hittable, not just its one
/// glyph cell), when `mode` has a pill. Cells outside the buffer are skipped.
pub fn icon_backdrop_in(
    mode: IconHover,
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    theme: crate::theme::Theme,
    hovered: bool,
) {
    if !hovered || mode != IconHover::Pill {
        return;
    }
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_bg(pill_bg(theme));
        }
    }
}

/// [`icon_backdrop_in`] under the `hover_icon` setting.
pub fn icon_backdrop(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    theme: crate::theme::Theme,
    hovered: bool,
) {
    icon_backdrop_in(icon_mode(), buf, area, theme, hovered);
}

/// [`icon_style_in`] under the `hover_icon` setting.
pub fn icon_style(theme: crate::theme::Theme, rest: Style, hovered: bool) -> Style {
    icon_style_in(icon_mode(), theme, rest, hovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn contains_is_inclusive_of_the_top_left_and_exclusive_of_the_far_edges() {
        let r = rect(5, 3, 10, 1);
        assert!(contains(r, Some((5, 3))), "top-left corner is inside");
        assert!(contains(r, Some((14, 3))), "last column is inside");
        assert!(!contains(r, Some((15, 3))), "one past the width is outside");
        assert!(!contains(r, Some((5, 4))), "one past the height is outside");
        assert!(!contains(r, Some((4, 3))), "one before the left is outside");
    }

    #[test]
    fn contains_is_false_without_a_pointer() {
        assert!(!contains(rect(0, 0, 4, 4), None));
    }

    #[test]
    fn row_hover_bg_is_theme_aware_and_only_fires_over_the_rect() {
        let r = rect(0, 0, 8, 1);
        assert_eq!(
            row_hover_bg(r, Some((2, 0)), crate::theme::Theme::DARK_BLUE),
            Some(ROW_HOVER_BG_DARK)
        );
        assert_eq!(
            row_hover_bg(r, Some((2, 0)), crate::theme::Theme::BLACK),
            Some(ROW_HOVER_BG_BLACK)
        );
        assert_eq!(
            row_hover_bg(r, Some((2, 5)), crate::theme::Theme::DARK_BLUE),
            None
        );
        assert_eq!(row_hover_bg(r, None, crate::theme::Theme::BLACK), None);
    }

    #[test]
    fn a_hovered_icon_follows_the_themes_own_accent_and_the_chosen_mode() {
        use crate::theme::Theme;
        let rest = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD);
        for theme in [Theme::DARK_BLUE, Theme::BLACK] {
            let acc = theme.accent();
            let pill = theme.tab_close_pill_bg();
            assert_eq!(icon_style_in(IconHover::Accent, theme, rest, false), rest);
            let s = icon_style_in(IconHover::Accent, theme, rest, true);
            assert_eq!((s.fg, s.bg), (Some(acc), None));
            assert!(s.add_modifier.contains(Modifier::BOLD), "bold is kept");
            let s = icon_style_in(IconHover::Color, theme, rest, true);
            assert_eq!((s.fg, s.bg), (Some(theme.ui(Color::White)), None));
            let s = icon_style_in(IconHover::Pill, theme, rest, true);
            assert_eq!((s.fg, s.bg), (Some(Color::Gray), Some(pill)));
        }
        assert_ne!(
            icon_hover_fg(Theme::DARK_BLUE),
            icon_hover_fg(Theme::BLACK),
            "each theme highlights in its own accent"
        );
    }

    #[test]
    fn the_pill_fills_the_whole_click_target_and_stays_inside_the_buffer() {
        use crate::theme::Theme;
        let theme = Theme::DARK_BLUE;
        let area = Rect::new(0, 0, 6, 1);
        let target = Rect::new(0, 0, 4, 1); // a glyph with a cell either side
        let bg = |buf: &ratatui::buffer::Buffer, x| buf[(x, 0)].bg;
        let mut buf = ratatui::buffer::Buffer::empty(area);
        icon_backdrop_in(IconHover::Pill, &mut buf, target, theme, true);
        for x in 0..4 {
            assert_eq!(bg(&buf, x), pill_bg(theme), "cell {x}");
        }
        assert_ne!(bg(&buf, 4), pill_bg(theme));
        let mut buf = ratatui::buffer::Buffer::empty(area);
        icon_backdrop_in(IconHover::Accent, &mut buf, target, theme, true);
        icon_backdrop_in(IconHover::Color, &mut buf, target, theme, true);
        icon_backdrop_in(IconHover::Pill, &mut buf, target, theme, false);
        assert_ne!(
            bg(&buf, 0),
            pill_bg(theme),
            "no pill unhovered or off the pill mode"
        );
        // A target that sticks out of the buffer is clipped, not a panic.
        icon_backdrop_in(
            IconHover::Pill,
            &mut buf,
            Rect::new(4, 0, 9, 3),
            theme,
            true,
        );
    }

    #[test]
    fn a_hovered_heading_takes_its_text_hover_colour_bold_or_a_pill() {
        use crate::theme::Theme;
        let theme = Theme::DARK_BLUE;
        let rest = Style::default().fg(Color::DarkGray);
        let up = Color::White;
        assert_eq!(
            heading_style_in(HeadingHover::Bold, theme, rest, up, false),
            rest
        );
        let s = heading_style_in(HeadingHover::Color, theme, rest, up, true);
        assert_eq!((s.fg, s.bg), (Some(up), None));
        assert!(!s.add_modifier.contains(Modifier::BOLD));
        let s = heading_style_in(HeadingHover::Bold, theme, rest, up, true);
        assert!(s.add_modifier.contains(Modifier::BOLD));
        let s = heading_style_in(HeadingHover::Pill, theme, rest, up, true);
        assert_eq!(s.bg, Some(theme.tab_close_pill_bg()));
    }

    #[test]
    fn the_hover_settings_parse_and_fall_back_to_the_default() {
        assert_eq!(HeadingHover::from_pref(""), HeadingHover::Color);
        assert_eq!(HeadingHover::from_pref("Bold"), HeadingHover::Bold);
        assert_eq!(HeadingHover::from_pref("pill"), HeadingHover::Pill);
        assert_eq!(HeadingHover::from_pref("nonsense"), HeadingHover::Color);
        assert_eq!(IconHover::from_pref(""), IconHover::Accent);
        assert_eq!(IconHover::from_pref("color"), IconHover::Color);
        assert_eq!(IconHover::from_pref("accent_pill"), IconHover::Accent);
        assert_eq!(IconHover::from_pref("pill"), IconHover::Pill);
    }
}
