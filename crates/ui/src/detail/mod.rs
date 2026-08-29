//! The right dock's commit detail panel: a tab bar switching between commit metadata and
//! the diff, above whichever of the two is selected. A second segmented bar sits beside it
//! while the diff is open, choosing between the unified and the side-by-side view.
//!
//! [`DetailPanel`] renders exactly the [`LoadState`] it is handed — it never reads a
//! repository itself. [`metadata::render_header`] and [`metadata::render_description`]
//! are the general-information tab's content — Subject, ID, Parents, Author and the
//! commit body. [`format`] holds the logic pulled out of [`metadata`] and [`diff`] so it
//! can be unit-tested without a window.
//!
//! Both tabs keep their scroll position in a [`ScrollHandle`] owned here rather than in
//! the element tree, which is rebuilt every render. The diff's handle is passed down to
//! the row element as well, because that element reads its own scroll offset to decide
//! which rows to lay out and paint — it is the only input it has to that decision before
//! the frame gives it any bounds.
//!
//! Two pieces of view state have to outlive a frame, and both for the same reason: a
//! render must not build them. `gpui-base` keys a window-level selection off a
//! [`TextSelectionHandle`], and a handle rebuilt per frame would drop the selection on
//! every repaint, so it is built once in [`DetailPanel::new`]. The diff's rows and the
//! string of every cell in them are derived from the patch, and deriving them per frame
//! would copy every line of the patch two or three times over on every repaint —
//! `refresh_window_on_change` repaints on each mouse move of a selection drag, so that is
//! not a rare frame. They are rebuilt by [`DetailPanel::set_detail`] and
//! [`DetailPanel::set_diff_view_mode`], which are the only two places the patch or the
//! view mode can change. [`DetailPanel::selected_tab`] is never touched by `set_detail`,
//! which is what lets picking a different commit leave the open tab alone.
//!
//! The diff view mode is read once from disk in [`DetailPanel::new`], before the first
//! frame, and written back through `cx.background_executor()`:
//! [`crate::persistence::save_diff_view_mode`] blocks on file I/O and a toggle is a frame
//! event, so it is saved the way [`crate::workspace::Workspace`] saves the theme rather
//! than inline.
//!
//! Both `set_detail` and a mode change route through `DetailPanel::reset_diff_view`, which
//! zeroes the diff's scroll offset and clears the window selection. Clearing is the reason
//! both take a `&mut Window`, which is why [`crate::workspace::Workspace`] threads one into
//! `sync_panels_from_repository`. It is not optional: `gpui-base` stores a selection
//! endpoint relative to the participant's registered origin (`text_selection.rs:1336`), so it
//! survives the content underneath it changing, and a stored `y` then resolves onto whatever row now sits
//! at that offset — a highlight over lines the user never dragged across, which Cmd-C would
//! copy. `TextSelection::clear` is window-wide rather than per-participant, and this panel
//! has participants beyond the diff body: [`metadata`] renders every value through
//! [`gpui_component::text::markdown`], and each `TextView` registers one of its own. They
//! are cleared too, which is the right outcome — they are selections over the commit that
//! is being replaced.

mod diff;
mod format;
mod metadata;

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Window, div, point, prelude::FluentBuilder as _,
    px,
};
use gpui_base::{AutoScroll, TextSelection, TextSelectionEvent, TextSelectionHandle};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    alert::Alert,
    dock::{Panel, PanelEvent},
    input::{Copy, SelectAll},
    scroll::ScrollableElement as _,
    spinner::Spinner,
    tab::{Tab, TabBar},
};

use crate::diff_view_mode::DiffViewMode;
use crate::persistence;
use crate::repository::{CommitDetail, LoadState};

use diff::DiffContent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DetailTab {
    #[default]
    General,
    Diff,
}

impl DetailTab {
    const ALL: [DetailTab; 2] = [DetailTab::General, DetailTab::Diff];

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    fn label(self) -> &'static str {
        match self {
            DetailTab::General => "General",
            DetailTab::Diff => "Diffs",
        }
    }
}

pub struct DetailPanel {
    detail: LoadState<Arc<CommitDetail>>,
    diff_content: Option<Rc<DiffContent>>,
    diff_selection: TextSelectionHandle,
    diff_select_all: bool,
    diff_auto_scroll: AutoScroll,
    diff_view_mode: DiffViewMode,
    selected_tab: DetailTab,
    general_scroll_handle: ScrollHandle,
    diff_scroll_handle: ScrollHandle,
    focus_handle: FocusHandle,
}

impl DetailPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let diff_selection = TextSelectionHandle::new("", cx);
        diff_selection.refresh_window_on_change(window, cx).detach();

        let focus_handle = cx.focus_handle();
        let focus = focus_handle.clone();
        diff_selection.focus_with(move |window, cx| focus.focus(window, cx), cx);

        let panel = cx.weak_entity();
        diff_selection
            .subscribe(
                move |event, cx| {
                    let _ = match event {
                        TextSelectionEvent::AutoScroll(delta) => {
                            let delta = *delta;
                            panel.update(cx, |panel, cx| panel.auto_scroll_diff(delta, cx))
                        }
                        TextSelectionEvent::Cleared => {
                            panel.update(cx, |panel, cx| panel.forget_select_all(cx))
                        }
                        TextSelectionEvent::SelectionChanged(_) => Ok(()),
                    };
                },
                cx,
            )
            .detach();

        Self {
            detail: LoadState::Idle,
            diff_content: None,
            diff_selection,
            diff_select_all: false,
            diff_auto_scroll: AutoScroll::default(),
            diff_view_mode: persistence::load_diff_view_mode().unwrap_or_default(),
            selected_tab: DetailTab::default(),
            general_scroll_handle: ScrollHandle::new(),
            diff_scroll_handle: ScrollHandle::new(),
            focus_handle,
        }
    }

    pub fn set_detail(
        &mut self,
        detail: LoadState<Arc<CommitDetail>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail = detail;
        self.rebuild_diff_content();
        self.reset_diff_view(window, cx);
        cx.notify();
    }

    fn set_diff_view_mode(
        &mut self,
        mode: DiffViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if mode == self.diff_view_mode {
            return;
        }
        self.diff_view_mode = mode;
        self.rebuild_diff_content();
        self.reset_diff_view(window, cx);

        cx.background_executor()
            .spawn(async move {
                if let Err(error) = persistence::save_diff_view_mode(&mode) {
                    eprintln!("gitr: failed to save diff view mode: {error:#}");
                }
            })
            .detach();

        cx.notify();
    }

    fn rebuild_diff_content(&mut self) {
        self.diff_content = match &self.detail {
            LoadState::Ready(detail) => {
                Some(Rc::new(diff::content(&detail.patch, self.diff_view_mode)))
            }
            _ => None,
        };
    }

    fn reset_diff_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_auto_scroll.stop();
        self.diff_select_all = false;
        self.diff_scroll_handle.set_offset(Point::default());
        TextSelection::clear(window, cx);
    }

    fn forget_select_all(&mut self, cx: &mut Context<Self>) {
        if !self.diff_select_all {
            return;
        }
        self.diff_select_all = false;
        cx.notify();
    }

    fn auto_scroll_diff(&mut self, delta: Option<Pixels>, cx: &mut Context<Self>) {
        self.diff_auto_scroll.set(delta, cx, |delta, panel, cx| {
            let offset = panel.diff_scroll_handle.offset();
            panel
                .diff_scroll_handle
                .set_offset(offset - point(px(0.), delta));
            cx.notify();
        });
    }

    fn on_copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        let text = TextSelection::selected_text(window, cx);
        if text.is_empty() {
            cx.propagate();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let selectable = self.selected_tab == DetailTab::Diff
            && self
                .diff_content
                .as_ref()
                .is_some_and(|content| !content.is_empty());
        if !selectable {
            cx.propagate();
            return;
        }

        self.diff_select_all = true;
        self.diff_selection.set_local_selection(true, cx);
        cx.notify();
    }
}

impl Panel for DetailPanel {
    fn panel_name(&self) -> &'static str {
        "DetailPanel"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Detail"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }
}

impl EventEmitter<PanelEvent> for DetailPanel {}

impl Focusable for DetailPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DetailPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_tab = self.selected_tab;
        let diff_view_mode = self.diff_view_mode;
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_select_all))
            .size_full()
            .flex()
            .flex_col()
            .child(tab_bar(selected_tab, diff_view_mode, cx))
            .child(match &self.detail {
                LoadState::Idle => centered_message(cx, "Select a commit to see its details."),
                LoadState::Loading => loading_state(cx),
                LoadState::Failed(message) => failed_state(message),
                LoadState::Ready(detail) => ready_state(
                    detail,
                    selected_tab,
                    self.diff_content.as_ref(),
                    self.diff_select_all,
                    &self.diff_selection,
                    &self.general_scroll_handle,
                    &self.diff_scroll_handle,
                    cx,
                ),
            })
    }
}

fn tab_bar(
    selected: DetailTab,
    diff_view_mode: DiffViewMode,
    cx: &mut Context<DetailPanel>,
) -> AnyElement {
    let mut tabs = TabBar::new("detail-tabs")
        .segmented()
        .small()
        .selected_index(selected.index())
        .on_click(cx.listener(|this, index: &usize, _, cx| {
            this.selected_tab = DetailTab::from_index(*index);
            cx.notify();
        }));
    for tab in DetailTab::ALL {
        tabs = tabs.child(Tab::new().label(tab.label()));
    }

    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(tabs)
        .when(selected == DetailTab::Diff, |row| {
            row.child(diff_view_mode_bar(diff_view_mode, cx))
        })
        .into_any_element()
}

fn diff_view_mode_bar(selected: DiffViewMode, cx: &mut Context<DetailPanel>) -> AnyElement {
    let mut modes = TabBar::new("diff-view-mode")
        .segmented()
        .small()
        .selected_index(selected.index())
        .on_click(cx.listener(|this, index: &usize, window, cx| {
            this.set_diff_view_mode(DiffViewMode::from_index(*index), window, cx);
        }));
    for mode in DiffViewMode::ALL {
        modes = modes.child(Tab::new().label(mode.label()));
    }
    modes.into_any_element()
}

fn centered_message(cx: &App, message: &str) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(message.to_string())
        .into_any_element()
}

fn loading_state(cx: &App) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .child(Spinner::new())
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child("Loading commit\u{2026}"),
        )
        .into_any_element()
}

fn failed_state(message: &str) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .p_3()
        .child(Alert::error("detail-panel-error", message.to_string()))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn ready_state(
    detail: &CommitDetail,
    selected_tab: DetailTab,
    diff_content: Option<&Rc<DiffContent>>,
    diff_select_all: bool,
    diff_selection: &TextSelectionHandle,
    general_scroll_handle: &ScrollHandle,
    diff_scroll_handle: &ScrollHandle,
    cx: &App,
) -> AnyElement {
    match selected_tab {
        DetailTab::General => general_tab(detail, general_scroll_handle, cx),
        DetailTab::Diff => diff_tab(
            diff_content,
            diff_select_all,
            diff_selection,
            diff_scroll_handle,
            cx,
        ),
    }
}

/// The whole tab scrolls as one region, header included, rather than pinning the header
/// and giving the body a short scroller of its own. A bounded inner scroller inside an
/// already-tall panel wastes the height it was given and cuts a long message mid-sentence
/// while empty space sits below it.
fn general_tab(detail: &CommitDetail, scroll_handle: &ScrollHandle, cx: &App) -> AnyElement {
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(
            div()
                .id("detail-general-scroll")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll_handle)
                .flex()
                .flex_col()
                .child(metadata::render_header(&detail.commit, cx))
                .children(metadata::render_description(&detail.commit, cx)),
        )
        .vertical_scrollbar(scroll_handle)
        .into_any_element()
}

fn diff_tab(
    content: Option<&Rc<DiffContent>>,
    select_all: bool,
    selection: &TextSelectionHandle,
    scroll_handle: &ScrollHandle,
    cx: &App,
) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(diff::render(
            content,
            select_all,
            selection,
            scroll_handle,
            cx,
        ))
        .into_any_element()
}
