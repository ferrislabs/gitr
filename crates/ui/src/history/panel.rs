//! [`HistoryPanel`]: the centre-dock view over a loaded [`History`].
//!
//! Renders whatever [`LoadState`] it is given — it never reads a repository itself. A
//! scope tab bar and a search box sit above a virtualised [`DataTable`]; selecting a row
//! or changing either control reports back through [`HistoryPanelEvent`] rather than
//! acting on the repository directly, which is a sibling's job.

use std::sync::Arc;

use domain::{HistoryScope, ObjectId, Reference};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Pixels, Render, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, ElementExt as _, Icon, IconName, Sizable as _,
    dock::{Panel, PanelEvent},
    h_flex,
    input::{Input, InputEvent, InputState},
    tab::{Tab, TabBar},
    table::{DataTable, TableEvent, TableState},
    v_flex,
};

use crate::branch_actions::Deletion;
use crate::density;
use crate::repository::model::{History, HistoryFilter, LoadState};
use crate::workspace::Workspace;

use super::delegate::HistoryTableDelegate;

/// Reported by [`HistoryPanel`] when the user acts on it.
///
/// Never emitted for a change [`HistoryPanel::set_history`] or
/// [`HistoryPanel::set_filter`] itself caused — those are inbound state, and echoing them
/// back out would loop.
pub enum HistoryPanelEvent {
    Selected(ObjectId),
    /// A row was double-clicked. Always accompanied by a [`Self::Selected`] for the
    /// same commit: [`TableState`] selects a row unconditionally on every left click,
    /// and only then, on the second click of a pair, emits the double-click on top
    /// of that same row.
    DoubleClicked(ObjectId),
    FilterChanged(HistoryFilter),
}

impl EventEmitter<HistoryPanelEvent> for HistoryPanel {}
impl EventEmitter<PanelEvent> for HistoryPanel {}

/// The scopes a [`HistoryPanel`] offers, given the branch its filter is currently
/// (or was last) scoped to.
///
/// Pure so the tab set is testable without a window: the panel never invents a branch to
/// scope by, it only ever offers `All`, `Local`, and whichever single reference it was
/// last handed through [`HistoryFilter::scope`].
fn scope_options(branch_scope: &Option<Reference>) -> Vec<HistoryScope> {
    let mut scopes = vec![HistoryScope::AllReferences, HistoryScope::LocalReferences];
    if let Some(reference) = branch_scope {
        scopes.push(HistoryScope::Single(reference.clone()));
    }
    scopes
}

/// The tab label for one scope.
fn scope_label(scope: &HistoryScope) -> String {
    match scope {
        HistoryScope::AllReferences => "Tous".to_string(),
        HistoryScope::LocalReferences => "Local".to_string(),
        HistoryScope::Single(reference) => reference.short_name(),
    }
}

pub struct HistoryPanel {
    table: Entity<TableState<HistoryTableDelegate>>,
    search_input: Entity<InputState>,
    filter: HistoryFilter,
    branch_scope: Option<Reference>,
    selected: Option<ObjectId>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl HistoryPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = HistoryTableDelegate::new();
        let table = cx.new(|cx| TableState::new(delegate, window, cx).sortable(false));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));

        let table_subscription = cx.subscribe_in(&table, window, Self::on_table_event);
        let search_subscription = cx.subscribe_in(&search_input, window, Self::on_search_event);

        Self {
            table,
            search_input,
            filter: HistoryFilter::default(),
            branch_scope: None,
            selected: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![table_subscription, search_subscription],
        }
    }

    pub fn set_history(&mut self, history: LoadState<Arc<History>>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_history(history);
            table.refresh(cx);
        });
        cx.notify();
    }

    pub fn set_head(
        &mut self,
        deletion: Deletion,
        commit: Option<ObjectId>,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_head(deletion, commit);
            table.refresh(cx);
        });
        cx.notify();
    }

    pub fn set_workspace(&mut self, workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_workspace(workspace);
            table.refresh(cx);
        });
        cx.notify();
    }

    /// Hands the table's own width to its delegate, so Subject can take the space the
    /// four fixed columns leave rather than stopping at a constant.
    ///
    /// Fed from `on_prepaint`, which runs on every frame: the delegate answers whether the
    /// width actually moved and nothing happens when it did not. Refreshing unconditionally
    /// here would notify on every prepaint, and a notify is what schedules the next one.
    fn set_available_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        let changed = self.table.update(cx, |table, cx| {
            let changed = table.delegate_mut().set_available_width(width);
            if changed {
                table.refresh(cx);
            }
            changed
        });
        if changed {
            cx.notify();
        }
    }

    /// Drops everything that belonged to the repository previously shown, including the
    /// remembered branch scope.
    ///
    /// [`Self::set_filter`] with a default filter is not enough: it clears the scope and
    /// the query but leaves `branch_scope`, so a branch tab from the previous project
    /// survives a switch and fails against the new repository when clicked.
    pub fn reset_for_new_repository(&mut self, cx: &mut Context<Self>) {
        self.branch_scope = None;
        self.set_filter(HistoryFilter::default(), cx);
    }

    pub fn set_filter(&mut self, filter: HistoryFilter, cx: &mut Context<Self>) {
        if let HistoryScope::Single(reference) = &filter.scope {
            self.branch_scope = Some(reference.clone());
        }

        let query_changed = filter.query != self.filter.query;
        self.filter = filter;

        if query_changed {
            let query = self.filter.query.clone();
            self.table.update(cx, |table, cx| {
                table.delegate_mut().set_query(query);
                table.refresh(cx);
            });
        }

        cx.notify();
    }

    pub fn selected(&self) -> Option<ObjectId> {
        self.selected
    }

    fn select_scope(&mut self, scope: HistoryScope, cx: &mut Context<Self>) {
        self.filter.scope = scope;
        cx.emit(HistoryPanelEvent::FilterChanged(self.filter.clone()));
        cx.notify();
    }

    fn on_table_event(
        &mut self,
        table: &Entity<TableState<HistoryTableDelegate>>,
        event: &TableEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TableEvent::SelectRow(row_ix) => {
                let Some(commit) = table.read(cx).delegate().commit_at(*row_ix) else {
                    return;
                };

                self.selected = Some(commit);
                cx.emit(HistoryPanelEvent::Selected(commit));
                cx.notify();
            }
            TableEvent::DoubleClickedRow(row_ix) => {
                let Some(commit) = table.read(cx).delegate().commit_at(*row_ix) else {
                    return;
                };

                cx.emit(HistoryPanelEvent::DoubleClicked(commit));
            }
            _ => {}
        }
    }

    fn on_search_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::Change = event else {
            return;
        };

        let query = input.read(cx).value().to_string();
        self.filter.query = query.clone();
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_query(query);
            table.refresh(cx);
        });
        cx.emit(HistoryPanelEvent::FilterChanged(self.filter.clone()));
        cx.notify();
    }
}

impl Panel for HistoryPanel {
    fn panel_name(&self) -> &'static str {
        "HistoryPanel"
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "History"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_query = self.search_input.read(cx).value().to_string();
        if current_query != self.filter.query {
            let query = self.filter.query.clone();
            self.search_input.update(cx, |input, cx| {
                input.set_value(query, window, cx);
            });
        }

        let scopes = scope_options(&self.branch_scope);
        let selected_index = scopes.iter().position(|scope| scope == &self.filter.scope);

        let mut tabs = TabBar::new("history-scope")
            .segmented()
            .small()
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(scope) = scope_options(&this.branch_scope).get(*index).cloned() {
                    this.select_scope(scope, cx);
                }
            }));
        if let Some(index) = selected_index {
            tabs = tabs.selected_index(index);
        }
        for scope in &scopes {
            tabs = tabs.child(Tab::new().label(scope_label(scope)));
        }

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(tabs)
                    .child(
                        Input::new(&self.search_input)
                            .small()
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).small())
                            .w(px(220.)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .on_prepaint({
                        let panel = cx.entity();
                        move |bounds, _, cx| {
                            panel.update(cx, |panel, cx| {
                                panel.set_available_width(bounds.size.width, cx)
                            })
                        }
                    })
                    .child(
                        DataTable::new(&self.table)
                            .stripe(true)
                            .bordered(false)
                            .with_size(density::TABLE_ROW_HEIGHT),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::BranchName;

    #[test]
    fn forgetting_the_branch_scope_drops_the_third_tab_it_offered() {
        let remembered = Some(Reference::LocalBranch(BranchName::new("main").unwrap()));

        assert_eq!(
            scope_options(&remembered).len(),
            3,
            "a remembered branch is offered as a third scope"
        );
        assert_eq!(
            scope_options(&None).len(),
            2,
            "clearing it must take that tab away, or a switch leaves the previous project's \
             branch offered against a repository that has no such ref"
        );
    }

    #[test]
    fn without_a_branch_scope_only_all_and_local_are_offered() {
        let scopes = scope_options(&None);
        assert_eq!(
            scopes,
            vec![HistoryScope::AllReferences, HistoryScope::LocalReferences]
        );
    }

    #[test]
    fn a_remembered_branch_becomes_a_third_option() {
        let reference = Reference::LocalBranch(BranchName::new("main").unwrap());
        let scopes = scope_options(&Some(reference.clone()));
        assert_eq!(
            scopes,
            vec![
                HistoryScope::AllReferences,
                HistoryScope::LocalReferences,
                HistoryScope::Single(reference),
            ]
        );
    }

    #[test]
    fn scope_labels_read_in_french_for_the_built_in_scopes() {
        assert_eq!(scope_label(&HistoryScope::AllReferences), "Tous");
        assert_eq!(scope_label(&HistoryScope::LocalReferences), "Local");
    }

    #[test]
    fn a_single_scope_labels_itself_with_the_references_short_name() {
        let reference = Reference::LocalBranch(BranchName::new("chantier/m1").unwrap());
        assert_eq!(scope_label(&HistoryScope::Single(reference)), "chantier/m1");
    }

    #[test]
    fn a_double_click_event_carries_the_double_clicked_commits_id() {
        let id: ObjectId = "1".repeat(40).parse().unwrap();
        let event = HistoryPanelEvent::DoubleClicked(id);

        let HistoryPanelEvent::DoubleClicked(carried) = event else {
            panic!("expected HistoryPanelEvent::DoubleClicked");
        };
        assert_eq!(carried, id);
    }
}
