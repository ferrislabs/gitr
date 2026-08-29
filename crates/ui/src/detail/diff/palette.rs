use domain::LineOrigin;
use gpui::{Hsla, rgb};
use gpui_component::{ThemeColor, ThemeMode};

const LIGHT_ADDITION_BACKGROUND: u32 = 0xdafbe1;
const LIGHT_DELETION_BACKGROUND: u32 = 0xffebe9;

/// A tint has to clear two bars, and only one of them is legibility.
///
/// The light pair is unusable under Catppuccin Frappé: it is pale, and the code above it
/// is painted in `theme.foreground`, `#c6d0f5` — pale on pale. But an earlier dark green
/// picked purely for legibility against that text landed at 1.00:1 against Frappé's own
/// `#303446` background: identical luminance, so the band was invisible and the tint may
/// as well not have been drawn. This value clears both — 1.58:1 against the background so
/// the band reads, 5.11:1 under `#c6d0f5` so the code stays legible on it.
const DARK_ADDITION_BACKGROUND: u32 = 0x355a40;

/// Mirrors [`DARK_ADDITION_BACKGROUND`]'s two-bar reasoning: 1.30:1 against Frappé's
/// background, 6.20:1 under the same `#c6d0f5`. A deletion can afford a darker plate than
/// an addition because nothing has to read *as* red on it — only the marker is tinted, and
/// it is drawn in `theme.red` rather than in the plate's own hue.
const DARK_DELETION_BACKGROUND: u32 = 0x5f3c45;

/// The plate a line of a given origin sits on, and the colour of its `+`/`−` marker.
///
/// `foreground` is the marker's colour alone. The code itself is painted in
/// `theme.foreground` whatever its origin — which is the measurement the two dark
/// constants above are chosen against.
pub(super) struct LineColors {
    pub background: Option<Hsla>,
    pub foreground: Hsla,
}

pub(super) fn line_colors(origin: LineOrigin, mode: ThemeMode, theme: &ThemeColor) -> LineColors {
    let background = match (origin, mode.is_dark()) {
        (LineOrigin::Context, _) => None,
        (LineOrigin::Addition, false) => Some(rgb(LIGHT_ADDITION_BACKGROUND).into()),
        (LineOrigin::Addition, true) => Some(rgb(DARK_ADDITION_BACKGROUND).into()),
        (LineOrigin::Deletion, false) => Some(rgb(LIGHT_DELETION_BACKGROUND).into()),
        (LineOrigin::Deletion, true) => Some(rgb(DARK_DELETION_BACKGROUND).into()),
    };
    let foreground = match origin {
        LineOrigin::Context => theme.muted_foreground,
        LineOrigin::Addition => theme.green,
        LineOrigin::Deletion => theme.red,
    };
    LineColors {
        background,
        foreground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_mode_uses_the_exact_given_hex_values() {
        let theme = ThemeColor::light();
        assert_eq!(
            line_colors(LineOrigin::Addition, ThemeMode::Light, &theme).background,
            Some(rgb(0xdafbe1).into())
        );
        assert_eq!(
            line_colors(LineOrigin::Deletion, ThemeMode::Light, &theme).background,
            Some(rgb(0xffebe9).into())
        );
    }

    #[test]
    fn a_marker_is_tinted_by_its_origin_rather_than_by_the_code_colour() {
        let theme = ThemeColor::dark();
        let added = line_colors(LineOrigin::Addition, ThemeMode::Dark, &theme).foreground;
        let deleted = line_colors(LineOrigin::Deletion, ThemeMode::Dark, &theme).foreground;
        let context = line_colors(LineOrigin::Context, ThemeMode::Dark, &theme).foreground;

        assert_eq!(added, theme.green);
        assert_eq!(deleted, theme.red);
        assert_eq!(context, theme.muted_foreground);
        assert_ne!(added, theme.foreground);
        assert_ne!(deleted, theme.foreground);
    }

    #[test]
    fn a_context_line_has_no_background() {
        let theme = ThemeColor::light();
        assert!(
            line_colors(LineOrigin::Context, ThemeMode::Light, &theme)
                .background
                .is_none()
        );
    }

    #[test]
    fn an_addition_and_a_deletion_do_not_share_a_background() {
        let theme = ThemeColor::light();
        let added = line_colors(LineOrigin::Addition, ThemeMode::Light, &theme).background;
        let deleted = line_colors(LineOrigin::Deletion, ThemeMode::Light, &theme).background;
        assert!(added.is_some() && deleted.is_some());
        assert_ne!(added, deleted);
    }

    #[test]
    fn dark_mode_does_not_reuse_the_light_pair() {
        let theme = ThemeColor::dark();
        let light = line_colors(LineOrigin::Addition, ThemeMode::Light, &theme).background;
        let dark = line_colors(LineOrigin::Addition, ThemeMode::Dark, &theme).background;
        assert_ne!(light, dark);
    }
}
