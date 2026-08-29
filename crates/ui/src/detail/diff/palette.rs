use domain::LineOrigin;
use gpui::{Hsla, rgb};
use gpui_component::ThemeMode;

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
/// an addition because nothing has to read *as* red on it.
const DARK_DELETION_BACKGROUND: u32 = 0x5f3c45;

/// The plate a line of a given origin sits on, and the whole of the signal that it is an
/// addition or a deletion. The code itself is painted in `theme.foreground` whatever its
/// origin — which is the measurement the two dark constants above are chosen against.
pub(super) fn line_background(origin: LineOrigin, mode: ThemeMode) -> Option<Hsla> {
    match (origin, mode.is_dark()) {
        (LineOrigin::Context, _) => None,
        (LineOrigin::Addition, false) => Some(rgb(LIGHT_ADDITION_BACKGROUND).into()),
        (LineOrigin::Addition, true) => Some(rgb(DARK_ADDITION_BACKGROUND).into()),
        (LineOrigin::Deletion, false) => Some(rgb(LIGHT_DELETION_BACKGROUND).into()),
        (LineOrigin::Deletion, true) => Some(rgb(DARK_DELETION_BACKGROUND).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_mode_uses_the_exact_given_hex_values() {
        assert_eq!(
            line_background(LineOrigin::Addition, ThemeMode::Light),
            Some(rgb(0xdafbe1).into())
        );
        assert_eq!(
            line_background(LineOrigin::Deletion, ThemeMode::Light),
            Some(rgb(0xffebe9).into())
        );
    }

    #[test]
    fn a_context_line_has_no_background() {
        assert!(line_background(LineOrigin::Context, ThemeMode::Light).is_none());
        assert!(line_background(LineOrigin::Context, ThemeMode::Dark).is_none());
    }

    #[test]
    fn an_addition_and_a_deletion_do_not_share_a_background() {
        let added = line_background(LineOrigin::Addition, ThemeMode::Light);
        let deleted = line_background(LineOrigin::Deletion, ThemeMode::Light);
        assert!(added.is_some() && deleted.is_some());
        assert_ne!(added, deleted);
    }

    #[test]
    fn dark_mode_does_not_reuse_the_light_pair() {
        let light = line_background(LineOrigin::Addition, ThemeMode::Light);
        let dark = line_background(LineOrigin::Addition, ThemeMode::Dark);
        assert_ne!(light, dark);
    }
}
