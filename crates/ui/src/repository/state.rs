//! The single owner of every repository read.
//!
//! [`RepositoryState`] is a gpui entity: it opens a repository through `vcs`, keeps the
//! [`crate::repository::model`] shapes views consume, and reissues reads when
//! [`FsRepositoryWatcher`] reports that something outside gitr moved. Every blocking `vcs`
//! call runs on `cx.background_executor()`; the frame thread only ever sees the already
//! computed result applied back through `cx.spawn`.
//!
//! Each read opens its own [`GixRepositoryReader`] rather than sharing one across the
//! concurrent head/references/history tasks: `gix::Repository` is `Send` but not `Sync` in
//! this build, so sharing one instance across tasks that can run in parallel would need a
//! mutex that serialises reads with no benefit over three independent, cheap opens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use domain::{
    Aspect, CommitSummary, HeadState, HistoryRequest, ObjectId, PatchReader, RefEntry, Reference,
    RepositoryChange, RepositoryReader, RepositoryWatcher,
};
use gpui::{Context, EventEmitter, Task};
use graph::{GraphLayout, layout};
use vcs::gix_reader::GixRepositoryReader;
use vcs::process::SubprocessPatchReader;
use vcs::watch::{FsRepositoryWatcher, WatchGuard};

use super::model::{
    CommitDetail, History, HistoryFilter, LoadState, ReferenceIndex, RepositoryEvent,
};

/// How often the watch task looks for a change the receiving thread has parked for it.
///
/// The wait is a `gpui` timer, which yields, never a blocking receive. An earlier version
/// awaited a `recv_timeout` on the executor instead, and that call landed on the frame
/// thread: it held the main queue for 150 ms out of every 150 ms, and the window never
/// opened at all. Nothing on an executor may block, which is the same rule the whole crate
/// is built on.
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct RepositoryState {
    path: PathBuf,
    head: LoadState<HeadState>,
    references: LoadState<ReferenceIndex>,
    history: LoadState<Arc<History>>,
    detail: LoadState<Arc<CommitDetail>>,
    filter: HistoryFilter,
    selected: Option<ObjectId>,
    head_task: Option<Task<()>>,
    references_task: Option<Task<()>>,
    history_task: Option<Task<()>>,
    detail_task: Option<Task<()>>,
    watch_task: Option<Task<()>>,
    watch_guard: Option<WatchGuard>,
}

impl RepositoryState {
    /// Opens `path` and starts its three initial reads. `watch` gates whether a
    /// filesystem watcher is also started: gitr's own remote cache is written only by
    /// gitr itself (a clone, or a synchronise fetch), so a watcher on it would only ever
    /// observe gitr's own writes and fire a reload gitr had already applied — a remote
    /// project's caller passes `false` and relies on an explicit synchronise instead. A
    /// local repository can change from outside gitr at any time, so its caller passes
    /// `true`.
    pub fn open(path: PathBuf, watch: bool, cx: &mut Context<Self>) -> Self {
        let mut state = Self {
            path,
            head: LoadState::Loading,
            references: LoadState::Loading,
            history: LoadState::Loading,
            detail: LoadState::Idle,
            filter: HistoryFilter::default(),
            selected: None,
            head_task: None,
            references_task: None,
            history_task: None,
            detail_task: None,
            watch_task: None,
            watch_guard: None,
        };
        state.reload_head(cx);
        state.reload_references(cx);
        state.reload_history(cx);
        if watch {
            state.start_watching(cx);
        }
        state
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn head(&self) -> &LoadState<HeadState> {
        &self.head
    }

    pub fn references(&self) -> &LoadState<ReferenceIndex> {
        &self.references
    }

    pub fn history(&self) -> &LoadState<Arc<History>> {
        &self.history
    }

    pub fn detail(&self) -> &LoadState<Arc<CommitDetail>> {
        &self.detail
    }

    pub fn filter(&self) -> &HistoryFilter {
        &self.filter
    }

    pub fn selected(&self) -> Option<ObjectId> {
        self.selected
    }

    pub fn set_filter(&mut self, filter: HistoryFilter, cx: &mut Context<Self>) {
        let reload = requires_reload(&self.filter, &filter);
        self.filter = filter;
        cx.notify();
        if reload {
            self.reload_history(cx);
        }
    }

    /// A no-op when `commit` is already selected, so clicking the same row twice does not
    /// restart an in-flight or already-finished detail load.
    pub fn select(&mut self, commit: Option<ObjectId>, cx: &mut Context<Self>) {
        if commit == self.selected {
            return;
        }
        self.selected = commit;
        match commit {
            Some(id) => self.start_detail_load(id, cx),
            None => self.clear_detail(cx),
        }
        cx.notify();
    }

    pub fn reload(&mut self, change: RepositoryChange, cx: &mut Context<Self>) {
        let targets = reload_targets(change);
        if targets.head {
            self.reload_head(cx);
        }
        if targets.references {
            self.reload_references(cx);
        }
        if targets.history {
            self.reload_history(cx);
        }
    }

    /// Replacing `head_task` drops whatever read was in flight, which cancels it
    /// immediately (`gpui::Task` semantics) — a later call always wins over an earlier one
    /// that has not yet applied its result.
    fn reload_head(&mut self, cx: &mut Context<Self>) {
        self.head.start_reload();
        cx.notify();
        let path = self.path.clone();
        self.head_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { read_head(&path) })
                .await;
            let _ = this.update(cx, |state, cx| state.apply_head(result, cx));
        }));
    }

    fn reload_references(&mut self, cx: &mut Context<Self>) {
        self.references.start_reload();
        cx.notify();
        let path = self.path.clone();
        self.references_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { read_references(&path) })
                .await;
            let _ = this.update(cx, |state, cx| state.apply_references(result, cx));
        }));
    }

    /// The graph layout is computed in the same background closure as the walk that
    /// produces the commits it lays out, never after the result reaches the foreground.
    fn reload_history(&mut self, cx: &mut Context<Self>) {
        self.history.start_reload();
        cx.notify();
        let path = self.path.clone();
        let scope = self.filter.scope.clone();
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let request = HistoryRequest { scope, limit: None };
            let result = cx
                .background_executor()
                .spawn(async move { read_history(&path, &request) })
                .await;
            let _ = this.update(cx, |state, cx| state.apply_history(result, cx));
        }));
    }

    fn start_detail_load(&mut self, id: ObjectId, cx: &mut Context<Self>) {
        self.detail = LoadState::Loading;
        cx.emit(RepositoryEvent::SelectionChanged);
        let path = self.path.clone();
        self.detail_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { read_commit_detail(&path, id) })
                .await;
            let _ = this.update(cx, |state, cx| state.apply_detail(id, result, cx));
        }));
    }

    fn clear_detail(&mut self, cx: &mut Context<Self>) {
        self.detail = LoadState::Idle;
        self.detail_task = None;
        cx.emit(RepositoryEvent::SelectionChanged);
    }

    fn apply_head(
        &mut self,
        result: Result<HeadState, domain::RepositoryError>,
        cx: &mut Context<Self>,
    ) {
        self.head = match result {
            Ok(head) => LoadState::Ready(head),
            Err(error) => failed(error.to_string(), cx),
        };
        cx.notify();
        cx.emit(RepositoryEvent::HistoryChanged);
    }

    fn apply_references(
        &mut self,
        result: Result<Vec<RefEntry>, domain::RepositoryError>,
        cx: &mut Context<Self>,
    ) {
        self.references = match result {
            Ok(entries) => LoadState::Ready(ReferenceIndex::from_entries(entries)),
            Err(error) => failed(error.to_string(), cx),
        };
        cx.notify();
        cx.emit(RepositoryEvent::HistoryChanged);
    }

    /// A commit that survives the fresh walk keeps its selection and its already loaded
    /// detail, since a commit's content is immutable and does not need re-reading; a
    /// selection that no longer resolves is cleared, taking `detail` down with it.
    fn apply_history(
        &mut self,
        result: Result<(Vec<CommitSummary>, GraphLayout), domain::RepositoryError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((commits, graph_layout)) => {
                let refs = self
                    .references
                    .ready()
                    .map(refs_by_commit)
                    .unwrap_or_default();
                let had_selection = self.selected.is_some();
                self.selected = preserve_selection(self.selected, &commits);
                self.history = LoadState::Ready(Arc::new(History {
                    commits,
                    layout: graph_layout,
                    refs_by_commit: refs,
                }));
                if had_selection && self.selected.is_none() {
                    self.clear_detail(cx);
                }
            }
            Err(error) => self.history = failed(error.to_string(), cx),
        }
        cx.notify();
        cx.emit(RepositoryEvent::HistoryChanged);
    }

    fn apply_detail(
        &mut self,
        id: ObjectId,
        result: Result<CommitDetail, String>,
        cx: &mut Context<Self>,
    ) {
        if self.selected != Some(id) {
            return;
        }
        self.detail = match result {
            Ok(detail) => LoadState::Ready(Arc::new(detail)),
            Err(message) => failed(message, cx),
        };
        cx.notify();
        cx.emit(RepositoryEvent::SelectionChanged);
    }

    /// Starts the watch off the frame thread and pumps its `std::sync::mpsc::Receiver`
    /// onto the gpui executor for as long as this task runs. Owning `watch_guard` on
    /// `self` — set once the watch is live — is what stops watching when the entity drops;
    /// this task then observes the resulting channel disconnect and exits on its own.
    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.watch_task = Some(cx.spawn(async move |this, cx| {
            let setup = cx
                .background_executor()
                .spawn(async move {
                    let (sender, receiver) = mpsc::channel();
                    FsRepositoryWatcher::new()
                        .watch(&path, sender)
                        .map(|guard| (guard, receiver))
                })
                .await;

            let (guard, receiver) = match setup {
                Ok(pair) => pair,
                Err(error) => {
                    let message: Arc<str> = error.to_string().into();
                    let _ = this.update(cx, |_, cx| cx.emit(RepositoryEvent::Failed(message)));
                    return;
                }
            };

            if this
                .update(cx, |state, _| state.watch_guard = Some(guard))
                .is_err()
            {
                return;
            }

            let mailbox = Arc::new(Mutex::new(WatchMailbox::default()));
            thread::spawn({
                let mailbox = Arc::clone(&mailbox);
                move || pump_changes(receiver, mailbox)
            });

            loop {
                cx.background_executor().timer(WATCH_POLL_INTERVAL).await;

                let collected = match mailbox.lock() {
                    Ok(mut mailbox) => (mailbox.pending.take(), mailbox.disconnected),
                    Err(_) => return,
                };

                if let Some(change) = collected.0
                    && this
                        .update(cx, |state, cx| state.reload(change, cx))
                        .is_err()
                {
                    return;
                }

                if collected.1 {
                    return;
                }
            }
        }));
    }
}

impl EventEmitter<RepositoryEvent> for RepositoryState {}

fn failed<T>(message: String, cx: &mut Context<RepositoryState>) -> LoadState<T> {
    let message: Arc<str> = message.into();
    cx.emit(RepositoryEvent::Failed(message.clone()));
    LoadState::Failed(message)
}

/// The four `read_*` functions below are the entire load path: each `reload_*` method
/// only wraps one in `LoadState` bookkeeping. They are `pub` — wider than anything else
/// in this module — because `gpui`'s `test-support` feature (needed for
/// `gpui::TestAppContext`, which is what driving `RepositoryState` itself through
/// `Context` in a test would require) is not enabled for this crate's `gpui` dependency,
/// confirmed empirically: `use gpui::TestAppContext;` from `crates/ui/tests/` fails to
/// resolve. `crates/ui/tests/repository_state.rs` calls these directly against real
/// repositories instead, since that is the closest available proof that this module
/// actually reads one.
pub fn read_head(path: &Path) -> Result<HeadState, domain::RepositoryError> {
    GixRepositoryReader::open(path)?.head()
}

pub fn read_references(path: &Path) -> Result<Vec<RefEntry>, domain::RepositoryError> {
    GixRepositoryReader::open(path)?.references()
}

pub fn read_history(
    path: &Path,
    request: &HistoryRequest,
) -> Result<(Vec<CommitSummary>, GraphLayout), domain::RepositoryError> {
    let commits = GixRepositoryReader::open(path)?.history(request)?;
    let graph_layout = layout(&commits);
    Ok((commits, graph_layout))
}

pub fn read_commit_detail(path: &Path, id: ObjectId) -> Result<CommitDetail, String> {
    let commit = GixRepositoryReader::open(path)
        .and_then(|reader| reader.commit(id))
        .map_err(|error| error.to_string())?;
    let patch = SubprocessPatchReader::new(path.to_path_buf())
        .commit_patch(id)
        .map_err(|error| error.to_string())?;
    Ok(CommitDetail { commit, patch })
}

/// What the receiving thread parks for the watch task to collect.
///
/// `pending` coalesces with [`RepositoryChange::merge`], so a burst of ref, index and
/// lock-file writes from one `git` command becomes one reload rather than several, however
/// many arrive between two polls.
#[derive(Default)]
struct WatchMailbox {
    pending: Option<RepositoryChange>,
    disconnected: bool,
}

/// Blocks on `receiver` for the life of the watch, on a thread of its own.
///
/// A dedicated thread rather than an executor task: `recv` blocks indefinitely, and gpui's
/// executors must never be blocked. The thread ends when the sender closes, which is what
/// dropping the watch guard does, so nothing else has to signal it.
fn pump_changes(receiver: mpsc::Receiver<RepositoryChange>, mailbox: Arc<Mutex<WatchMailbox>>) {
    while let Ok(change) = receiver.recv() {
        let Ok(mut mailbox) = mailbox.lock() else {
            return;
        };
        mailbox.pending = Some(match mailbox.pending {
            Some(existing) => existing.merge(change),
            None => change,
        });
    }
    if let Ok(mut mailbox) = mailbox.lock() {
        mailbox.disconnected = true;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ReloadTargets {
    head: bool,
    references: bool,
    history: bool,
}

/// `Aspect::Head` and `Aspect::References` each stale all three of head, references and
/// history. References is not narrower than Head: a plain `git commit` on an attached
/// branch rewrites only `refs/heads/<branch>` — classified `Aspect::References` by
/// `crates/vcs/src/watch/classify.rs` — and never touches `.git/HEAD` itself, so treating
/// `Aspect::Head` as the only trigger for a `head` reload would leave the resolved branch
/// target stale after every commit made through a terminal. `Aspect::Index` and
/// `Aspect::WorkingTree` stale nothing read here.
fn reload_targets(change: RepositoryChange) -> ReloadTargets {
    let stale = change.contains(Aspect::Head) || change.contains(Aspect::References);
    ReloadTargets {
        head: stale,
        references: stale,
        history: stale,
    }
}

fn preserve_selection(selected: Option<ObjectId>, commits: &[CommitSummary]) -> Option<ObjectId> {
    selected.filter(|id| commits.iter().any(|commit| commit.id == *id))
}

fn requires_reload(current: &HistoryFilter, next: &HistoryFilter) -> bool {
    current.scope != next.scope
}

fn refs_by_commit(index: &ReferenceIndex) -> HashMap<ObjectId, Vec<Reference>> {
    let mut map: HashMap<ObjectId, Vec<Reference>> = HashMap::new();
    for entry in index
        .local_branches
        .iter()
        .chain(index.remote_branches.iter())
        .chain(index.tags.iter())
    {
        map.entry(entry.target)
            .or_default()
            .push(entry.reference.clone());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{BranchName, HistoryScope, Parents, RemoteName, Signature, TagName, Timestamp};

    fn id(nibble: char) -> ObjectId {
        nibble.to_string().repeat(40).parse().unwrap()
    }

    fn commit(nibble: char) -> CommitSummary {
        CommitSummary {
            id: id(nibble),
            parents: Parents::Root,
            summary: String::new(),
            author: Signature {
                name: String::new(),
                email: String::new(),
                time: Timestamp {
                    seconds: 0,
                    offset_minutes: 0,
                },
            },
        }
    }

    fn ref_entry(reference: Reference, target: char) -> RefEntry {
        RefEntry {
            reference,
            target: id(target),
            upstream: None,
        }
    }

    #[test]
    fn head_aspect_stales_head_references_and_history() {
        let targets = reload_targets(RepositoryChange::only(Aspect::Head));
        assert_eq!(
            targets,
            ReloadTargets {
                head: true,
                references: true,
                history: true,
            }
        );
    }

    #[test]
    fn references_aspect_stales_head_references_and_history() {
        let targets = reload_targets(RepositoryChange::only(Aspect::References));
        assert_eq!(
            targets,
            ReloadTargets {
                head: true,
                references: true,
                history: true,
            }
        );
    }

    #[test]
    fn index_aspect_stales_nothing() {
        assert_eq!(
            reload_targets(RepositoryChange::only(Aspect::Index)),
            ReloadTargets::default()
        );
    }

    #[test]
    fn working_tree_aspect_stales_nothing() {
        assert_eq!(
            reload_targets(RepositoryChange::only(Aspect::WorkingTree)),
            ReloadTargets::default()
        );
    }

    #[test]
    fn a_checkout_shaped_change_stales_everything_because_head_is_present() {
        let change = RepositoryChange::only(Aspect::Head)
            .with(Aspect::Index)
            .with(Aspect::WorkingTree);
        assert_eq!(
            reload_targets(change),
            ReloadTargets {
                head: true,
                references: true,
                history: true,
            }
        );
    }

    #[test]
    fn index_and_working_tree_together_still_stale_nothing() {
        let change = RepositoryChange::only(Aspect::Index).with(Aspect::WorkingTree);
        assert_eq!(reload_targets(change), ReloadTargets::default());
    }

    #[test]
    fn a_present_selection_survives_a_reload_that_still_contains_it() {
        let commits = vec![commit('a'), commit('b')];
        assert_eq!(preserve_selection(Some(id('b')), &commits), Some(id('b')));
    }

    #[test]
    fn a_selection_is_cleared_when_the_reload_no_longer_contains_it() {
        let commits = vec![commit('a')];
        assert_eq!(preserve_selection(Some(id('b')), &commits), None);
    }

    #[test]
    fn no_selection_stays_no_selection() {
        let commits = vec![commit('a')];
        assert_eq!(preserve_selection(None, &commits), None);
    }

    #[test]
    fn changing_the_scope_requires_a_reload() {
        let current = HistoryFilter::default();
        let next = HistoryFilter {
            scope: HistoryScope::LocalReferences,
            ..HistoryFilter::default()
        };
        assert!(requires_reload(&current, &next));
    }

    #[test]
    fn changing_only_the_query_does_not_require_a_reload() {
        let current = HistoryFilter::default();
        let next = HistoryFilter {
            query: "fix".into(),
            ..HistoryFilter::default()
        };
        assert!(!requires_reload(&current, &next));
    }

    #[test]
    fn refs_by_commit_groups_every_kind_by_its_target() {
        let local = Reference::LocalBranch(BranchName::new("main").unwrap());
        let remote = Reference::RemoteBranch {
            remote: RemoteName::new("origin").unwrap(),
            branch: BranchName::new("main").unwrap(),
        };
        let tag = Reference::Tag(TagName::new("v1.0.0").unwrap());

        let mut index = ReferenceIndex::default();
        index.local_branches.push(ref_entry(local, 'a'));
        index.remote_branches.push(ref_entry(remote, 'a'));
        index.tags.push(ref_entry(tag.clone(), 'b'));

        let map = refs_by_commit(&index);
        assert_eq!(map.get(&id('a')).map(Vec::len), Some(2));
        assert_eq!(map.get(&id('b')), Some(&vec![tag]));
        assert_eq!(map.get(&id('c')), None);
    }
}
