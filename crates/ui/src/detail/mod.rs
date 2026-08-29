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
//! The one piece of view state that does need to outlive a frame is the diff's selection
//! participant: `gpui-base` keys a window-level selection off a
//! [`TextSelectionHandle`], and a handle rebuilt per frame would drop the selection on
//! every repaint. It is built once in [`DetailPanel::new`], which is also the only place
//! with the `&Window` its refresh subscription needs. Nothing else here is staged:
//! [`DetailPanel::set_detail`] stores the new [`LoadState`] and notifies, and the rows are
//! derived from the patch during the render that follows.
//! [`DetailPanel::selected_tab`] is never touched by `set_detail`, which is what lets
//! picking a different commit leave the open tab alone.
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
//! endpoint relative to `bounds.origin` (`text_selection.rs:1336`), so it survives the
//! content underneath it changing, and a stored `y` then resolves onto whatever row now sits
//! at that offset — a highlight over lines the user never dragged across, which Cmd-C would
//! copy. `TextSelection::clear` is window-wide rather than per-participant, which costs
//! nothing here because the diff body is the only participant this crate registers.

mod diff;
mod format;
mod metadata;

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Point, Render, ScrollHandle, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_base::{TextSelection, TextSelectionHandle};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    alert::Alert,
    dock::{Panel, PanelEvent},
    scroll::ScrollableElement as _,
    spinner::Spinner,
    tab::{Tab, TabBar},
};

use crate::diff_view_mode::DiffViewMode;
use crate::persistence;
use crate::repository::{CommitDetail, LoadState};

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
    diff_selection: TextSelectionHandle,
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
        Self {
            detail: LoadState::Idle,
            diff_selection,
            diff_view_mode: persistence::load_diff_view_mode().unwrap_or_default(),
            selected_tab: DetailTab::default(),
            general_scroll_handle: ScrollHandle::new(),
            diff_scroll_handle: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_detail(
        &mut self,
        detail: LoadState<Arc<CommitDetail>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail = detail;
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

    fn reset_diff_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_scroll_handle.set_offset(Point::default());
        TextSelection::clear(window, cx);
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
                    diff_view_mode,
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

fn ready_state(
    detail: &CommitDetail,
    selected_tab: DetailTab,
    diff_view_mode: DiffViewMode,
    diff_selection: &TextSelectionHandle,
    general_scroll_handle: &ScrollHandle,
    diff_scroll_handle: &ScrollHandle,
    cx: &App,
) -> AnyElement {
    match selected_tab {
        DetailTab::General => general_tab(detail, general_scroll_handle, cx),
        DetailTab::Diff => diff_tab(
            detail,
            diff_view_mode,
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
    detail: &CommitDetail,
    mode: DiffViewMode,
    selection: &TextSelectionHandle,
    scroll_handle: &ScrollHandle,
    cx: &App,
) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .child(diff::render(
            &detail.patch,
            mode,
            selection,
            scroll_handle,
            cx,
        ))
        .into_any_element()
}
