//! Shared section-header pill button (the small codicon affordances VS Code
//! paints at the right of a view's title row). Used by the Explorer, Open
//! Editors, Source Control, Remote Explorer and Search headers, so the look of
//! an interactive header icon (bold glyph, blue on hover, brand-teal pill under
//! the Black theme) stays defined in exactly one place.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// Background colour painted under a header pill button so it reads as a touch
/// target, not a naked single glyph. Matches the chip behind the view title.
pub const HEADER_BTN_BG: Color = Color::Rgb(0x1e, 0x3a, 0x6e);
const HEADER_BTN_FG: Color = Color::Rgb(0xe6, 0xed, 0xf5);
/// Codicon `cod-refresh` — the circular-arrow glyph VS Code paints on a view's
/// refresh affordance. Verified against the Nerd Font cmap.
pub const REFRESH_GLYPH: char = '\u{eb37}';
pub const ADD_GLYPH: char = '\u{ea60}';
/// Codicon `cod-new_file` — the document-with-plus glyph VS Code paints on the
/// Explorer title's "New File" action. Verified against Nerd Fonts
/// `glyphnames.json` (`cod-new_file` = U+EA7F); do NOT use the upstream
/// `vscode-codicons/mapping.json` value, which targets a different PUA range.
pub const NEW_FILE_GLYPH: char = '\u{ea7f}';
/// Codicon `cod-new_folder` — the folder-with-plus glyph for "New Folder".
/// Nerd Fonts `cod-new_folder` = U+EA80.
pub const NEW_FOLDER_GLYPH: char = '\u{ea80}';
/// Codicon `cod-save_all` — the stacked-disks glyph VS Code paints on the
/// OPEN EDITORS header's "Save All" action (#852). Nerd Fonts `cod-save_all`
/// = U+EB49 (verified against `glyphnames.json`; `cod-save` is U+EB4B).
pub const SAVE_ALL_GLYPH: char = '\u{eb49}';
/// Codicon `cod-collapse_all` — the stacked-rows-with-minus glyph for
/// "Collapse Folders in Explorer". Nerd Fonts `cod-collapse_all` = U+EAC5.
pub const COLLAPSE_ALL_GLYPH: char = '\u{eac5}';
/// The "Views and More Actions" (⋯) affordance VS Code paints at the right of a
/// view-container header, as a space-padded action label so it reads as a real
/// pill inset from the rounded corner — the same `" + "` treatment the terminal
/// pane buttons wear. The glyph is the *midline* horizontal ellipsis (U+22EF
/// `⋯`), NOT the baseline U+2026 `…`: the midline variant centres its dots on
/// the line so they sit level with the EXPLORER caps and the `+`, instead of
/// dropping to the baseline. It is a standard Misc-Math-Symbols codepoint, not a
/// codicon PUA glyph, so it still renders on every terminal/font — the same
/// no-codicon-dependency reasoning behind the U+2715 close glyph.
pub const MORE_LABEL: &str = " \u{22ef} ";

/// Paint a header pill button at cell `(x, y)`. Under the Black theme (`brand`)
/// it is a chipless light icon that grows a faint teal pill only while hovered.
/// Croft Dark paints no chip either: the bold glyph is light at rest and lifts
/// to the accent blue on hover.
///
/// The painted footprint is exactly **one cell** — the single-width codicon.
/// A wider highlight around a 1-cell glyph always left a bare cell beside the
/// icon, which read as a misalignment. Callers keep a wider hit-test rect for a
/// forgiving click target; only the visible highlight is snug.
pub fn render(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    glyph: char,
    theme: crate::theme::Theme,
    hovered: bool,
) {
    // VS Code paints view-action icons in a light `icon.foreground`, not an
    // accent hue; on hover [`hover::icon_style`] takes over, in the active
    // theme's own accent.
    let rest = Style::default()
        .add_modifier(Modifier::BOLD)
        .fg(icon_rest_fg(theme));
    // The click targets are two cells, the glyph and the one after it; the
    // pill shows exactly that, so neighbouring icons never share a cell.
    let target = Rect::new(x, y, 2, 1);
    crate::widgets::hover::icon_backdrop(buf, target, theme, hovered);
    buf.set_string(
        x,
        y,
        glyph.to_string(),
        crate::widgets::hover::icon_style(theme, rest, hovered),
    );
}

/// The resting colour of a header icon: the near-white the panel title wears
/// under the Black theme, `icon.foreground` on the others.
fn icon_rest_fg(theme: crate::theme::Theme) -> Color {
    if theme.gradient() {
        crate::gradient::rgb_color(crate::gradient::PANEL_TITLE_FG)
    } else {
        theme.ui(HEADER_BTN_FG)
    }
}

/// The shared style for a header *action* pill — the space-padded `" + "` /
/// `" ⋯ "` affordances that sit on a pane's top border. This is the single
/// source of truth the terminal pane buttons and the Explorer `⋯` button both
/// draw from, so the two can never drift apart again:
///
/// * Croft Dark (`!brand`): white-on-navy chip, bold (the historical look).
/// * Black (`brand`): a chipless light icon in the VS Code toolbar spirit
///   (`icon.foreground`), no bold.
/// * Hovered: [`crate::widgets::hover::icon_style`], the same highlight every
///   interactive icon gets (the theme's accent and/or the toolbar pill, per the
///   `hover_icon` setting).
pub fn action_style(theme: crate::theme::Theme, hovered: bool) -> Style {
    let rest = if theme.gradient() {
        Style::default().fg(icon_rest_fg(theme))
    } else {
        Style::default()
            .fg(theme.ui(Color::White))
            .bg(theme.ui(HEADER_BTN_BG))
            .add_modifier(Modifier::BOLD)
    };
    crate::widgets::hover::icon_style(theme, rest, hovered)
}

/// Paint a space-padded action label (e.g. `" ⋯ "`) as a header pill whose top
/// edge sits inset one cell from the pane's rounded corner, and return its
/// hit-test rect. This is the design the terminal `+` / `-` buttons use; the
/// Explorer `⋯` shares it verbatim via [`action_style`]. `right_edge` is the
/// pane's right border column (`area.x + area.width - 1`); the label is laid
/// out ending one cell short of it so the rounded corner stays intact.
///
/// Returns `None` when the row is too narrow to seat the padded label without
/// colliding with the corner.
pub fn render_action(
    buf: &mut Buffer,
    right_edge: u16,
    y: u16,
    label: &str,
    theme: crate::theme::Theme,
    pointer: Option<(u16, u16)>,
) -> Option<Rect> {
    let w = label.chars().count() as u16;
    if right_edge < w + 1 {
        return None;
    }
    let x = right_edge - w; // leave the corner cell (right_edge) for the border
    let hovered = pointer.is_some_and(|(px, py)| py == y && px >= x && px < x + w);
    buf.set_string(x, y, label, action_style(theme, hovered));
    Some(Rect {
        x,
        y,
        width: w,
        height: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_icon_is_bold_light_and_lifts_in_the_theme_accent_on_hover() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 1,
        };
        for theme in [crate::theme::Theme::DARK_BLUE, crate::theme::Theme::BLACK] {
            let mut buf = Buffer::empty(area);
            render(&mut buf, 2, 0, REFRESH_GLYPH, theme, false);
            assert_eq!(buf[(2, 0)].symbol().chars().next(), Some(REFRESH_GLYPH));
            assert_eq!(buf[(2, 0)].bg, Color::Reset, "no chip at rest");
            assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD));
            let rest_fg = buf[(2, 0)].fg;
            let mut buf = Buffer::empty(area);
            render(&mut buf, 2, 0, REFRESH_GLYPH, theme, true);
            assert_eq!(buf[(2, 0)].fg, theme.accent(), "the theme's own accent");
            assert_ne!(buf[(2, 0)].fg, rest_fg);
            // Only the glyph's own cell is painted.
            assert_eq!(buf[(1, 0)].fg, Color::Reset);
            assert_eq!(buf[(3, 0)].fg, Color::Reset);
        }
    }
}
