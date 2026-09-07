//! Reference badges shown inline before a commit's subject.
//!
//! A badge is coloured by [`BadgeKind`], not by the reference's identity, so every local
//! branch reads the same way and a remote branch or tag never gets mistaken for one.
//!
//! Shared by the history table, where badges sit before a commit's subject, and by the
//! detail panel, where the same badges name the branches a commit belongs to. The colour
//! mapping is the whole point of the module: the two views must not drift apart, or the
//! same branch reads as two different things depending on where it is looked at.

use gpui::{
    App, Hsla, IntoElement, ParentElement as _, Pixels, RenderOnce, TextRun, Window, px, rgb,
};
use gpui_component::{Selectable, Sizable as _, ThemeColor, tag::Tag};

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

/// [`Tag`]'s own metrics at `xsmall`, read off `gpui_component::tag`: `text_xs`,
/// `px_1p5` on each side and `border_1` all round.
///
/// Duplicated here because measuring a badge means predicting what `Tag` will render, and
/// `Tag` exposes no measurement of its own. Kept in rems rather than pixels for the two
/// that are: `Tag` writes them as rem-based utilities, so they follow `Window::rem_size`
/// and a hardcoded pixel count would quietly mismeasure at any other root size.
const TAG_FONT_REMS: f32 = 0.75;
const TAG_PADDING_REMS: f32 = 0.375;
const TAG_BORDER: f32 = 1.;

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

/// How wide the badge for `label` will render.
///
/// Shapes the text through the window's own text system rather than estimating from a
/// character count: a branch name is proportional text, and `origin/dependabot/npm_and_yarn`
/// is nowhere near thirty times the width of an `i`. The shaped line is cached by gpui, so
/// asking once per badge per visible row is not a per-frame layout pass.
pub fn measure_badge(label: &str, window: &Window) -> Pixels {
    let rem = window.rem_size();
    let font_size = rem * TAG_FONT_REMS;

    let style = window.text_style();
    let run = TextRun {
        len: label.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let text = window
        .text_system()
        .layout_line(label, font_size, &[run], None)
        .width;

    text + rem * TAG_PADDING_REMS * 2. + px(TAG_BORDER * 2.)
}

/// How many of `widths` fit whole inside `budget`, the rest being counted into a single
/// overflow badge of width `overflow`.
///
/// A badge either renders entirely or does not render at all — the point of the count is
/// that no branch name is ever cut mid-word. `overflow` is reserved whenever anything is
/// left over, and only then: a set that fits gets the whole budget.
///
/// Walks down from "everything fits" rather than up from nothing, because dropping a badge
/// can *free* space — the last one to go takes the overflow reservation away with it — so
/// the largest k that fits is not always reachable by adding badges one at a time.
///
/// Two rules then override the arithmetic, and both exist because a counter is worth less
/// than what it replaces — it says a branch is there without saying which.
///
/// **The first badge always renders.** A row reduced to a counter alone has lost the one
/// name it had room to say. Whatever the budget, the leading reference is drawn.
///
/// **A counter never stands for a single reference.** A `+1` takes nearly the width of the
/// badge it replaces, so where exactly one would be left over the reference itself is
/// drawn instead.
///
/// Both let the strip overrun `budget`. That is deliberate and bounded: the budget is half
/// the subject cell, so an overrun still lands inside it, and the subject beside it
/// truncates — which is the ordinary thing for a subject to do.
pub fn fitting_badge_count(
    widths: &[Pixels],
    budget: Pixels,
    gap: Pixels,
    overflow: Pixels,
) -> usize {
    if widths.is_empty() {
        return 0;
    }

    let mut shown = 0;
    for candidate in (1..=widths.len()).rev() {
        let mut needed: Pixels = widths[..candidate].iter().copied().sum();
        needed += gap * (candidate - 1) as f32;

        if candidate < widths.len() {
            needed += overflow + gap;
        }

        if needed <= budget {
            shown = candidate;
            break;
        }
    }

    // The leading badge is not negotiable, and a counter left standing for one reference
    // is replaced by that reference. Applied in this order: forcing the first badge can
    // itself leave exactly one behind.
    shown = shown.max(1);
    if widths.len() - shown == 1 {
        shown = widths.len();
    }

    shown
}

/// The label of the badge standing in for `hidden` references that did not fit.
pub fn overflow_label(hidden: usize) -> String {
    format!("+{hidden}")
}

/// The overflow badge — how many references the strip could not show whole.
///
/// Deliberately in the muted foreground rather than in any of the four
/// [`BadgeKind`] colours: it stands for a mixed set, and painting it green would claim the
/// hidden references are local branches.
///
/// A type of its own rather than a plain element because it is a popover trigger, and
/// `Popover::trigger` takes `Selectable` — it wants to mark the trigger while the popover
/// is open. Nothing here reads that flag: a badge has no selected appearance, and inventing
/// one would make the strip's colours mean two different things at once.
#[derive(IntoElement)]
pub struct OverflowBadge {
    hidden: usize,
    color: Hsla,
    selected: bool,
}

impl OverflowBadge {
    pub fn new(hidden: usize, theme: &ThemeColor) -> Self {
        Self {
            hidden,
            color: theme.muted_foreground,
            selected: false,
        }
    }
}

impl Selectable for OverflowBadge {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for OverflowBadge {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        Tag::custom(
            self.color.opacity(0.12),
            self.color,
            self.color.opacity(0.3),
        )
        .rounded(px(BADGE_RADIUS))
        .xsmall()
        .child(overflow_label(self.hidden))
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

    fn widths(values: &[f32]) -> Vec<Pixels> {
        values.iter().copied().map(px).collect()
    }

    #[test]
    fn a_set_that_fits_shows_every_badge_and_no_counter() {
        let shown = fitting_badge_count(&widths(&[100., 80.]), px(200.), px(4.), px(30.));
        assert_eq!(shown, 2, "184 of a 200 budget: nothing overflows");
    }

    #[test]
    fn a_badge_that_would_be_cropped_is_dropped_whole() {
        let shown = fitting_badge_count(&widths(&[100., 80., 80.]), px(150.), px(4.), px(30.));
        assert_eq!(
            shown, 1,
            "the second badge needs 84 more and only 50 are left, so it goes entirely \
             rather than being cut"
        );
    }

    #[test]
    fn the_last_badge_that_fits_still_loses_to_the_counter() {
        assert_eq!(
            fitting_badge_count(&widths(&[100., 80., 80.]), px(130.), px(4.), px(30.)),
            1,
            "134 is what one badge plus its counter costs and only 130 are on offer, so \
             the arithmetic gives nothing — but the leading badge renders regardless"
        );
    }

    #[test]
    fn a_set_that_exactly_fills_the_budget_needs_no_counter() {
        assert_eq!(
            fitting_badge_count(&widths(&[100., 80.]), px(184.), px(4.), px(30.)),
            2,
            "100 + 4 + 80 is the budget to the pixel, and nothing overflows, so no room \
             is reserved for a counter that would not be drawn"
        );
    }

    #[test]
    fn a_lone_leftover_is_shown_rather_than_counted() {
        assert_eq!(
            fitting_badge_count(&widths(&[100., 80.]), px(150.), px(4.), px(30.)),
            2,
            "the second badge does not fit, but a `+1` in its place costs nearly as much \
             and names nothing — so it overruns the budget instead"
        );
    }

    #[test]
    fn the_first_badge_renders_however_narrow_the_budget() {
        assert_eq!(
            fitting_badge_count(&widths(&[300., 300., 300.]), px(10.), px(4.), px(30.)),
            1,
            "a row shrunk to a counter alone has lost the one name it had room to say"
        );
    }

    #[test]
    fn a_single_reference_too_wide_for_the_budget_is_still_shown() {
        assert_eq!(
            fitting_badge_count(&widths(&[300.]), px(60.), px(4.), px(30.)),
            1,
            "a row whose only badge is replaced by `+1` has lost the branch name and \
             gained nothing — this is the case the counter must never take"
        );
    }

    #[test]
    fn no_references_need_no_room() {
        assert_eq!(fitting_badge_count(&[], px(0.), px(4.), px(30.)), 0);
    }

    #[test]
    fn the_overflow_label_counts_what_is_hidden_not_what_is_shown() {
        assert_eq!(overflow_label(1), "+1");
        assert_eq!(overflow_label(12), "+12");
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
