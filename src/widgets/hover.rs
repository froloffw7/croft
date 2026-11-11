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
use ratatui::style::{Color, Style};
use std::cell::Cell;

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
    /// The text brightens to a soft white (the default).
    #[default]
    Soft,
    /// The text takes the colour of `hover_heading_color`.
    Color,
    /// The text takes the theme's accent.
    Accent,
    /// The soft white on a pill of the theme's accent.
    Pill,
}

impl HeadingHover {
    /// Parse the setting; anything unknown (or an older config's empty
    /// string) is the default.
    pub fn from_pref(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "color" => Self::Color,
            "accent" => Self::Accent,
            "pill" => Self::Pill,
            _ => Self::Soft,
        }
    }
}

/// How a hovered *icon* (header pill glyphs, the open-tabs list button, the
/// tab close cross, the layout toggles, `+` / `-` / `⋯`) shows the pointer:
/// the `hover_icon` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconHover {
    /// The glyph takes the theme's accent (the default).
    #[default]
    Accent,
    /// The glyph takes the colour of `hover_icon_color`.
    Color,
    /// The glyph keeps its colour and gets a pill of the theme's accent.
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

/// A colour the user picks for the `color` hover modes: one of the active
/// theme's 16 terminal colours by name, or a fixed `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverColor {
    /// An index into [`crate::theme::Theme::ansi`].
    Ansi(usize),
    Rgb(u8, u8, u8),
}

/// The names `hover_heading_color` / `hover_icon_color` take, in ANSI order.
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

impl HoverColor {
    /// Parse the setting: an ANSI name (`bright_cyan`; `gray` is
    /// `bright_black`) or `#rrggbb`. Anything else, an empty string included,
    /// is none, and the mode falls back to the accent.
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        if let Some(hex) = t.strip_prefix('#')
            && hex.len() == 6
            && let Ok(v) = u32::from_str_radix(hex, 16)
        {
            return Some(Self::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
        }
        if t == "gray" || t == "grey" {
            return Some(Self::Ansi(8));
        }
        ANSI_NAMES.iter().position(|n| *n == t).map(Self::Ansi)
    }

    /// The colour under `theme`, whose own palette the names index.
    pub fn resolve(self, theme: crate::theme::Theme) -> Color {
        match self {
            Self::Ansi(i) => {
                let (r, g, b) = theme.ansi()[i.min(15)];
                Color::Rgb(r, g, b)
            }
            Self::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }
}

/// The hover settings as the draw code reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoverSettings {
    pub heading: HeadingHover,
    pub icon: IconHover,
    pub heading_color: Option<HoverColor>,
    pub icon_color: Option<HoverColor>,
    /// The `pill` fill of a heading / an icon; the accent when unset.
    pub heading_pill: Option<HoverColor>,
    pub icon_pill: Option<HoverColor>,
}

impl HoverSettings {
    /// From the string settings: the two modes, then the two `color`s, then
    /// the two pill fills.
    pub fn from_prefs(modes: (&str, &str), colors: (&str, &str), pills: (&str, &str)) -> Self {
        Self {
            heading: HeadingHover::from_pref(modes.0),
            icon: IconHover::from_pref(modes.1),
            heading_color: HoverColor::parse(colors.0),
            icon_color: HoverColor::parse(colors.1),
            heading_pill: HoverColor::parse(pills.0),
            icon_pill: HoverColor::parse(pills.1),
        }
    }
}

thread_local! {
    static SETTINGS: Cell<HoverSettings> = const {
        Cell::new(HoverSettings {
            heading: HeadingHover::Soft,
            icon: IconHover::Accent,
            heading_color: None,
            icon_color: None,
            heading_pill: None,
            icon_pill: None,
        })
    };
}

/// Apply the hover settings. Held per thread (the UI's), like the Tab Size, so
/// the hundreds of draw sites need no extra argument and a test that picks a
/// mode cannot disturb another.
pub fn set_settings(s: HoverSettings) {
    SETTINGS.with(|c| c.set(s));
}

/// Pick the two modes, leaving both colours unset (the accent).
#[cfg(test)]
pub fn set_modes(heading: HeadingHover, icon: IconHover) {
    set_settings(HoverSettings {
        heading,
        icon,
        ..HoverSettings::default()
    });
}

fn settings() -> HoverSettings {
    SETTINGS.with(Cell::get)
}

thread_local! {
    static POPUP_BORDER: Cell<ratatui::widgets::BorderType> =
        const { Cell::new(ratatui::widgets::BorderType::Plain) };
}

/// The border of the hint popups (LSP hover, signature help, completion, the
/// UI tooltips): the `popup_border` setting, `plain` (the default) or
/// `rounded` / `double` / `thick`.
pub fn popup_border_type() -> ratatui::widgets::BorderType {
    POPUP_BORDER.with(Cell::get)
}

/// Apply the `popup_border` setting; anything unknown is plain.
pub fn set_popup_border(pref: &str) {
    use ratatui::widgets::BorderType;
    let t = match pref.trim().to_ascii_lowercase().as_str() {
        "rounded" => BorderType::Rounded,
        "double" => BorderType::Double,
        "thick" => BorderType::Thick,
        _ => BorderType::Plain,
    };
    POPUP_BORDER.with(|c| c.set(t));
}

/// The soft white a hovered heading takes by default (the Black theme's own
/// panel-title tone there), softer than any accent.
pub fn text_hover_fg(theme: crate::theme::Theme) -> Color {
    if theme.gradient() {
        crate::gradient::rgb_color(crate::gradient::PANEL_TITLE_FG)
    } else {
        theme.ui(Color::White)
    }
}

/// The colour an interactive icon takes under the pointer: the active
/// theme's accent, so every theme highlights in its own hue.
pub fn icon_hover_fg(theme: crate::theme::Theme) -> Color {
    theme.accent()
}

/// The pill under a hovered heading or icon: the active theme's accent, the
/// same hue the `accent` modes colour the glyph with.
pub fn pill_bg(theme: crate::theme::Theme) -> Color {
    theme.accent()
}

/// The fill of a pill: the user's colour, the accent when none is set.
fn pill_fill(theme: crate::theme::Theme, custom: Option<HoverColor>) -> Color {
    custom.map_or_else(|| pill_bg(theme), |c| c.resolve(theme))
}

/// The user's colour for a `color` mode, the accent when none is set.
fn custom_fg(theme: crate::theme::Theme, custom: Option<HoverColor>) -> Color {
    custom.map_or_else(|| icon_hover_fg(theme), |c| c.resolve(theme))
}

/// `fg` as it is drawn on the hover pill. A high contrast theme (#1548) takes
/// whichever of black or white reads better on its pill (black on High
/// Contrast's orange); the other themes keep their own ink.
fn ink_on_pill(
    theme: crate::theme::Theme,
    fg: Color,
    pill: Color,
    custom: Option<HoverColor>,
) -> Color {
    if custom.is_some()
        && let Color::Rgb(r, g, b) = pill
    {
        // A fill the user chose: whichever of black or white reads better.
        return if crate::theme::contrast_ratio((r, g, b), (0, 0, 0))
            > crate::theme::contrast_ratio((r, g, b), (0xff, 0xff, 0xff))
        {
            Color::Black
        } else {
            Color::White
        };
    }
    match pill {
        Color::Rgb(r, g, b)
            if theme.high_contrast()
                && crate::theme::contrast_ratio((r, g, b), (0, 0, 0))
                    > crate::theme::contrast_ratio((r, g, b), (0xff, 0xff, 0xff)) =>
        {
            Color::Black
        }
        _ if theme.high_contrast() => Color::White,
        _ => fg,
    }
}

/// `rest` for a heading as drawn while hovered per `mode`; `hover_fg` is the
/// heading's soft hover colour and `custom` the user's `color`.
pub fn heading_style_in(
    mode: HeadingHover,
    custom: (Option<HoverColor>, Option<HoverColor>),
    theme: crate::theme::Theme,
    rest: Style,
    hover_fg: Color,
    hovered: bool,
) -> Style {
    let (custom, pill_custom) = custom;
    if !hovered {
        return rest;
    }
    match mode {
        HeadingHover::Soft => rest.fg(hover_fg),
        HeadingHover::Color => rest.fg(custom_fg(theme, custom)),
        HeadingHover::Accent => rest.fg(icon_hover_fg(theme)),
        HeadingHover::Pill => {
            let pill = pill_fill(theme, pill_custom);
            rest.fg(ink_on_pill(theme, hover_fg, pill, pill_custom))
                .bg(pill)
        }
    }
}

/// [`heading_style_in`] under the hover settings.
pub fn heading_style(
    theme: crate::theme::Theme,
    rest: Style,
    hover_fg: Color,
    hovered: bool,
) -> Style {
    let s = settings();
    let colors = (s.heading_color, s.heading_pill);
    heading_style_in(s.heading, colors, theme, rest, hover_fg, hovered)
}

/// `rest` for an icon as drawn while hovered per `mode`; `custom` is the
/// user's `color`.
pub fn icon_style_in(
    mode: IconHover,
    custom: (Option<HoverColor>, Option<HoverColor>),
    theme: crate::theme::Theme,
    rest: Style,
    hovered: bool,
) -> Style {
    let (custom, pill_custom) = custom;
    if !hovered {
        return rest;
    }
    match mode {
        IconHover::Accent => rest.fg(icon_hover_fg(theme)),
        IconHover::Color => rest.fg(custom_fg(theme, custom)),
        IconHover::Pill => {
            let pill = pill_fill(theme, pill_custom);
            let fg = rest
                .fg
                .map_or(Color::White, |c| ink_on_pill(theme, c, pill, pill_custom));
            rest.fg(fg).bg(pill)
        }
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
    pill: Option<HoverColor>,
    hovered: bool,
) {
    if !hovered || mode != IconHover::Pill {
        return;
    }
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_bg(pill_fill(theme, pill));
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
    icon_backdrop_in(
        settings().icon,
        buf,
        area,
        theme,
        settings().icon_pill,
        hovered,
    );
}

/// [`icon_style_in`] under the `hover_icon` setting.
pub fn icon_style(theme: crate::theme::Theme, rest: Style, hovered: bool) -> Style {
    let s = settings();
    icon_style_in(s.icon, (s.icon_color, s.icon_pill), theme, rest, hovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

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
            let pill = pill_bg(theme);
            assert_eq!(pill, acc, "the pill is the accent too");
            assert_eq!(
                icon_style_in(IconHover::Accent, (None, None), theme, rest, false),
                rest
            );
            let s = icon_style_in(IconHover::Accent, (None, None), theme, rest, true);
            assert_eq!((s.fg, s.bg), (Some(acc), None));
            assert!(s.add_modifier.contains(Modifier::BOLD), "bold is kept");
            // `color` without a colour falls back to the accent.
            let s = icon_style_in(IconHover::Color, (None, None), theme, rest, true);
            assert_eq!((s.fg, s.bg), (Some(acc), None));
            let red = HoverColor::parse("red");
            let s = icon_style_in(IconHover::Color, (red, None), theme, rest, true);
            assert_eq!(s.fg, Some(red.unwrap().resolve(theme)));
            let s = icon_style_in(IconHover::Pill, (None, None), theme, rest, true);
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
        icon_backdrop_in(IconHover::Pill, &mut buf, target, theme, None, true);
        for x in 0..4 {
            assert_eq!(bg(&buf, x), pill_bg(theme), "cell {x}");
        }
        assert_ne!(bg(&buf, 4), pill_bg(theme));
        let mut buf = ratatui::buffer::Buffer::empty(area);
        icon_backdrop_in(IconHover::Accent, &mut buf, target, theme, None, true);
        icon_backdrop_in(IconHover::Color, &mut buf, target, theme, None, true);
        icon_backdrop_in(IconHover::Pill, &mut buf, target, theme, None, false);
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
            None,
            true,
        );
    }

    #[test]
    fn a_hovered_heading_takes_soft_white_a_colour_the_accent_or_a_pill() {
        use crate::theme::Theme;
        let theme = Theme::DARK_BLUE;
        let rest = Style::default().fg(Color::DarkGray);
        let up = Color::White;
        let cyan = HoverColor::parse("bright_cyan");
        assert_eq!(
            heading_style_in(HeadingHover::Soft, (None, None), theme, rest, up, false),
            rest
        );
        let s = heading_style_in(HeadingHover::Soft, (cyan, None), theme, rest, up, true);
        assert_eq!((s.fg, s.bg), (Some(up), None), "soft ignores the colour");
        let s = heading_style_in(HeadingHover::Color, (cyan, None), theme, rest, up, true);
        assert_eq!((s.fg, s.bg), (Some(cyan.unwrap().resolve(theme)), None));
        let s = heading_style_in(HeadingHover::Color, (None, None), theme, rest, up, true);
        assert_eq!(s.fg, Some(theme.accent()), "unset falls back to accent");
        let s = heading_style_in(HeadingHover::Pill, (None, None), theme, rest, up, true);
        assert_eq!((s.fg, s.bg), (Some(up), Some(theme.accent())));
        let s = heading_style_in(HeadingHover::Accent, (None, None), theme, rest, up, true);
        assert_eq!((s.fg, s.bg), (Some(theme.accent()), None));
    }

    #[test]
    fn the_popup_border_parses_and_falls_back_to_plain() {
        use ratatui::widgets::BorderType;
        for (pref, want) in [
            ("", BorderType::Plain),
            ("Rounded", BorderType::Rounded),
            ("double", BorderType::Double),
            ("thick", BorderType::Thick),
            ("wavy", BorderType::Plain),
        ] {
            set_popup_border(pref);
            assert_eq!(popup_border_type(), want, "{pref:?}");
        }
        set_popup_border("");
    }

    #[test]
    fn a_chosen_pill_fill_replaces_the_accent_and_its_ink_follows_it() {
        use crate::theme::Theme;
        let theme = Theme::DARK_BLUE;
        let rest = Style::default().fg(Color::Gray);
        let yellow = HoverColor::parse("#ffee00");
        let s = icon_style_in(IconHover::Pill, (None, yellow), theme, rest, true);
        assert_eq!(s.bg, Some(Color::Rgb(0xff, 0xee, 0x00)));
        assert_eq!(s.fg, Some(Color::Black), "dark ink on a light fill");
        let navy = HoverColor::parse("#000080");
        let s = heading_style_in(
            HeadingHover::Pill,
            (None, navy),
            theme,
            rest,
            Color::Gray,
            true,
        );
        assert_eq!(s.bg, Some(Color::Rgb(0, 0, 0x80)));
        assert_eq!(s.fg, Some(Color::White), "light ink on a dark fill");
        let mut buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 2, 1));
        icon_backdrop_in(
            IconHover::Pill,
            &mut buf,
            Rect::new(0, 0, 2, 1),
            theme,
            yellow,
            true,
        );
        assert_eq!(buf[(1, 0)].bg, Color::Rgb(0xff, 0xee, 0x00));
    }

    #[test]
    fn the_hover_settings_parse_and_fall_back_to_the_default() {
        assert_eq!(HeadingHover::from_pref(""), HeadingHover::Soft);
        assert_eq!(HeadingHover::from_pref("Color"), HeadingHover::Color);
        assert_eq!(HeadingHover::from_pref("pill"), HeadingHover::Pill);
        assert_eq!(HeadingHover::from_pref("accent"), HeadingHover::Accent);
        assert_eq!(HeadingHover::from_pref("bold"), HeadingHover::Soft);
        assert_eq!(IconHover::from_pref(""), IconHover::Accent);
        assert_eq!(IconHover::from_pref("color"), IconHover::Color);
        assert_eq!(IconHover::from_pref("pill"), IconHover::Pill);
        assert_eq!(IconHover::from_pref("bold"), IconHover::Accent);
    }

    #[test]
    fn a_hover_colour_is_an_ansi_name_of_the_theme_or_a_hex_value() {
        use crate::theme::Theme;
        assert_eq!(HoverColor::parse("red"), Some(HoverColor::Ansi(1)));
        assert_eq!(
            HoverColor::parse(" Bright-Cyan "),
            Some(HoverColor::Ansi(14))
        );
        assert_eq!(HoverColor::parse("gray"), Some(HoverColor::Ansi(8)));
        assert_eq!(
            HoverColor::parse("#ff8000"),
            Some(HoverColor::Rgb(255, 128, 0))
        );
        for bad in ["", "#fff", "#gggggg", "teal"] {
            assert_eq!(HoverColor::parse(bad), None, "{bad:?}");
        }
        for (i, n) in ANSI_NAMES.iter().enumerate() {
            assert_eq!(HoverColor::parse(n), Some(HoverColor::Ansi(i)));
        }
        let (r, g, b) = Theme::BLACK.ansi()[2];
        assert_eq!(
            HoverColor::Ansi(2).resolve(Theme::BLACK),
            Color::Rgb(r, g, b)
        );
        let s = HoverSettings::from_prefs(("pill", "color"), ("red", "#102030"), ("blue", ""));
        assert_eq!(s.heading, HeadingHover::Pill);
        assert_eq!(s.icon, IconHover::Color);
        assert_eq!(s.heading_color, Some(HoverColor::Ansi(1)));
        assert_eq!(s.icon_color, Some(HoverColor::Rgb(0x10, 0x20, 0x30)));
        assert_eq!(s.heading_pill, Some(HoverColor::Ansi(4)));
        assert_eq!(s.icon_pill, None);
    }
}
