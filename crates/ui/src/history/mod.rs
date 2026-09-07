//! The history view: a virtualised commit table with a graph gutter, reference badges
//! and a search box.
//!
//! [`HistoryPanel`] is the only public surface. It renders whatever
//! [`crate::repository::model::LoadState`] it is handed and never reads a repository —
//! see the module docs on [`panel`] for the exact contract.

mod delegate;
mod format;
mod geometry;
mod panel;

pub use panel::{HistoryPanel, HistoryPanelEvent};
