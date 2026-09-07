//! Reference badges shown inline before a commit's subject.
//!
//! A badge is coloured by [`BadgeKind`], not by the reference's identity, so every local
//! branch reads the same way and a remote branch or tag never gets mistaken for one.
//!
//! Shared by the history table, where badges sit before a commit's subject, and by the
//! detail panel, where the same badges name the branches a commit belongs to. The colour
//! mapping is the whole point of the module: the two views must not drift apart, or the
//! same branch reads as two different things depending on where it is looked at.

use gpui::{Hsla, IntoElement, ParentElement as _, px, rgb};
use gpui_component::{Sizable as _, ThemeColor, tag::Tag};

use domain::{BranchName, Reference};

/// No theme in the tree carries an orange, so the checked-out branch brings its own.
/// Between `theme.yellow` and this sits the only pair the distinguishability tests come
/// close to failing, which is why the hue is pushed this far from red.
const CURRENT_BRANCH: u32 = 0xe8622a;

/// A tag's own yellow, in place of `theme.yellow`.
///
/// That token resolves to `#ca8a04` under the light theme — an amber dark enough to read
/// as brown beside the orange above, which is what made a tag hard to tell from the
/// checked-out branch. This is lighter and five degrees further round the wheel, which is
/// as far toward a pure yellow as the badge can go: the plate is the colour at 16% and the
/// text is the colour at full strength, so on a light background a truly yellow yellow
/// stops being legible long before it stops being yellow. Measured against the plate it
/// composites onto, the contrast is 2.0 in light mode and 5.1 in dark, against 2.7 for the
/// orange it has to be told apart from.
const TAG: u32 = 0xc9a227;

const BADGE_RADIUS: f32 = 4.;

/// Which of the four badge colours a reference gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeKind {
    CurrentBranch,
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// Classifies `reference` for badge colouring.
///
/// `head_branch` is the branch HEAD points at, or `None` on a detached HEAD — where no
/// local branch is current and every one of them reads as an ordinary local branch.
pub fn classify(reference: &Reference, head_branch: Option<&BranchName>) -> BadgeKind {
    match reference {
        Reference::LocalBranch(name) if head_branch == Some(name) => BadgeKind::CurrentBranch,
        Reference::LocalBranch(_) => BadgeKind::LocalBranch,
        Reference::RemoteBranch { .. } => BadgeKind::RemoteBranch,
        Reference::Tag(_) => BadgeKind::Tag,
    }
}

/// The theme colour a badge of `kind` renders with.
pub fn badge_color(kind: BadgeKind, theme: &ThemeColor) -> Hsla {
    match kind {
        BadgeKind::CurrentBranch => rgb(CURRENT_BRANCH).into(),
        BadgeKind::LocalBranch => theme.green,
        BadgeKind::RemoteBranch => theme.blue,
        BadgeKind::Tag => rgb(TAG).into(),
    }
}

/// One reference badge, as it sits inline before a subject.
pub fn render_badge(
    reference: &Reference,
    head_branch: Option<&BranchName>,
    theme: &ThemeColor,
) -> impl IntoElement {
    let color = badge_color(classify(reference, head_branch), theme);

    Tag::custom(color.opacity(0.16), color, color.opacity(0.4))
        .rounded(px(BADGE_RADIUS))
        .xsmall()
        .child(reference.short_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{BranchName, RemoteName, TagName};

    fn local() -> Reference {
        Reference::LocalBranch(BranchName::new("main").unwrap())
    }

    fn remote() -> Reference {
        Reference::RemoteBranch {
            remote: RemoteName::new("origin").unwrap(),
            branch: BranchName::new("main").unwrap(),
        }
    }

    fn tag() -> Reference {
        Reference::Tag(TagName::new("v1.0.0").unwrap())
    }

    #[test]
    fn classifies_each_reference_kind() {
        let head = BranchName::new("main").unwrap();
        assert_eq!(classify(&local(), None), BadgeKind::LocalBranch);
        assert_eq!(classify(&local(), Some(&head)), BadgeKind::CurrentBranch);
        assert_eq!(classify(&remote(), Some(&head)), BadgeKind::RemoteBranch);
        assert_eq!(classify(&tag(), Some(&head)), BadgeKind::Tag);
    }

    #[test]
    fn every_kind_is_distinguishable_in_light_mode() {
        assert_all_distinct(&ThemeColor::light());
    }

    #[test]
    fn every_kind_is_distinguishable_in_dark_mode() {
        assert_all_distinct(&ThemeColor::dark());
    }

    #[test]
    fn every_kind_is_distinguishable_under_gitr_light() {
        assert_all_distinct(&crate::theme_palette::resolve_for_tests(
            crate::theme_palette::LIGHT_THEME_NAME,
        ));
    }

    #[test]
    fn every_kind_is_distinguishable_under_catppuccin_frappe() {
        assert_all_distinct(&crate::theme_palette::resolve_for_tests(
            crate::theme_palette::DARK_THEME_NAME,
        ));
    }

    /// Same threshold and reasoning as `graph_palette`'s: raw `Hsla` inequality does not
    /// catch a badge colour that composites to nearly the background or to another
    /// badge's colour, only compositing first does.
    const MIN_DISTINGUISHABLE: f32 = 0.06;

    fn assert_all_distinct(theme: &ThemeColor) {
        let colors = [
            badge_color(BadgeKind::CurrentBranch, theme),
            badge_color(BadgeKind::LocalBranch, theme),
            badge_color(BadgeKind::RemoteBranch, theme),
            badge_color(BadgeKind::Tag, theme),
        ];
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate().skip(i + 1) {
                let distance = crate::theme_palette::rendered_distance(theme.background, *a, *b);
                assert!(
                    distance > MIN_DISTINGUISHABLE,
                    "badge kinds {i} and {j} read as the same colour once painted (distance {distance:.3})"
                );
            }
        }
    }
}
