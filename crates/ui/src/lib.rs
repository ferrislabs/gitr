//! Views built on gpui-component.
//!
//! Ports are synchronous and blocking, so every call into `vcs` from here runs on
//! `cx.background_executor()`. Nothing in this crate blocks the frame thread, and no
//! long operation opens a modal — that pairing is what makes GitX crash on
//! `assert(currentModalSheet == nil)` when two network operations overlap.
//!
//! [`workspace::Workspace`] is the window's root view. It owns a
//! [`repository::RepositoryState`], the single point where this crate reads a
//! `domain::RepositoryReader`, and pushes what it loads into [`history::HistoryPanel`] and
//! [`detail::DetailPanel`] — see `install_default_layout` in `workspace` for exactly
//! where those two panels plug into the dock.

pub mod actions;
pub mod branch_actions;
pub mod density;
pub mod detail;
pub mod diff_view_mode;
pub mod graph_palette;
pub mod history;
pub mod persistence;
pub mod project;
pub mod repository;
pub mod sidebar;
mod theme_palette;
pub mod theme_preference;
pub mod workspace;

pub use workspace::Workspace;

use gpui::{App, AppContext as _};
use gpui_component::dock::register_panel;

use detail::DetailPanel;
use history::HistoryPanel;

/// Registers everything this crate owns in the global `App` state.
///
/// Must run after `gpui_component::init(cx)` and before a saved dock layout is loaded:
/// `gpui_component::init` is what installs the `Theme` global this function's first two
/// calls override, and this teaches `gpui_component`'s `PanelRegistry` how to rebuild
/// [`HistoryPanel`] and [`DetailPanel`] from persisted JSON — a layout referencing an
/// unregistered panel name silently falls back to `gpui_component`'s `InvalidPanel`.
///
/// [`theme_palette::install`] runs before [`density::apply`], not after: both write to
/// the `Theme` global, `theme_palette::install` calls `Theme::change` to apply its
/// palette immediately, and only running density second guarantees its font size, mono
/// font size and radius are the values still in place when the window opens, regardless
/// of what a future edit to either theme JSON does.
pub fn init(cx: &mut App) {
    theme_palette::install(cx);
    density::apply(cx);

    register_panel(cx, "HistoryPanel", |_, _, _, window, cx| {
        Box::new(cx.new(|cx| HistoryPanel::new(window, cx)))
    });
    register_panel(cx, "DetailPanel", |_, _, _, window, cx| {
        Box::new(cx.new(|cx| DetailPanel::new(window, cx)))
    });
}
