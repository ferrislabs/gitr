use domain::LineOrigin;
use gpui::{Hsla, rgb};
use gpui_component::{ThemeColor, ThemeMode};

const LIGHT_ADDITION_BACKGROUND: u32 = 0xdafbe1;
const LIGHT_DELETION_BACKGROUND: u32 = 0xffebe9;

/// A tint has to clear two bars, and only one of them is legibility.
///
/// The light pair is unreadable under Catppuccin Frappé — its addition syntax colour
/// `#a6d189` sits at 1.56:1 against [`LIGHT_ADDITION_BACKGROUND`], pale on pale. But an
/// earlier dark green picked purely for legibility against that text landed at 1.00:1
/// against Frappé's own `#303446` background: identical luminance, so the band was
/// invisible and the tint may as well not have been drawn. This value clears both — 1.58:1
/// against the background so the band reads, 4.50:1 under the text so the code stays
/// legible on it.
const DARK_ADDITION_BACKGROUND: u32 = 0x355a40;

/// Mirrors [`DARK_ADDITION_BACKGROUND`]'s two-bar reasoning for Frappé's deletion syntax
/// colour `#e78284`: 1.30:1 against the background, 3.57:1 under the text. Red text is
/// lighter than green here, so the two bars pull harder against each other and this sits
/// where they meet.
const DARK_DELETION_BACKGROUND: u32 = 0x5f3c45;

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
    LineColors {
        background,
        foreground: theme.foreground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
