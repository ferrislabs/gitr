use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use domain::{
    BranchName, Commit, CommitSummary, HistoryScope, ObjectId, Patch, RefEntry, Reference,
};
use graph::GraphLayout;

/// Where an asynchronous read has got to.
///
/// `Failed` carries a rendered message rather than the error itself: the views that display
/// it cannot act on a typed error, and threading one through would make every view depend on
/// every adapter's error enum.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LoadState<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(Arc<str>),
}

impl<T> LoadState<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    pub fn start_reload(&mut self) {
        if self.ready().is_none() {
            *self = Self::Loading;
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(message) => Some(message),
            _ => None,
        }
    }
}

/// One loaded history, with everything a row needs to render already computed.
///
/// The graph layout and the reference index are built once per load rather than per frame.
/// `layout.rows` is parallel to `commits`, so row `i` of the table draws `layout.rows[i]`
/// without a lookup.
#[derive(Clone, Debug, Default)]
pub struct History {
    pub commits: Vec<CommitSummary>,
    pub layout: GraphLayout,
    /// References pointing at each commit, for the badges beside a subject. A commit with
    /// no reference is absent rather than mapped to an empty vector.
    pub refs_by_commit: HashMap<ObjectId, Vec<Reference>>,
}

impl History {
    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub fn references_at(&self, commit: ObjectId) -> &[Reference] {
        self.refs_by_commit
            .get(&commit)
            .map_or(&[], |references| references.as_slice())
    }

    /// The references a commit belongs under — what the detail panel names it by.
    ///
    /// A tip answers with its own references, which is the same set the history table
    /// draws beside its subject. Anything else answers with the nearest references found
    /// by walking *down the first-parent chain*: from the commit to its children whose
    /// **first** parent it is, and on until a level carries a reference.
    ///
    /// First-parent is what makes the answer read as "the branch this commit was written
    /// on". A commit made on a feature branch is the first parent of the next feature
    /// commit, and the branch's merge into main takes it as a *second* parent — so the
    /// walk climbs the feature branch to its tip and stops rather than spilling onto main.
    ///
    /// The rejected alternative is every branch that contains the commit, the
    /// `git branch --contains` set. It is the honest answer to a different question and
    /// unusable here: an old commit on the trunk is contained by every branch cut since,
    /// which on the repository this was built against is thirty badges above a subject.
    /// Stopping at the first level that carries a reference keeps the answer to the one
    /// or two branches that actually claim the commit.
    ///
    /// Walked on demand rather than indexed at load: it costs one pass over `commits` to
    /// build the child map, and it runs when a selection changes, not per frame.
    pub fn owning_references(&self, commit: ObjectId) -> Vec<Reference> {
        let at_commit = self.references_at(commit);
        if !at_commit.is_empty() {
            return at_commit.to_vec();
        }

        let mut first_children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
        for candidate in &self.commits {
            if let Some(first_parent) = candidate.parents.iter().next() {
                first_children
                    .entry(first_parent)
                    .or_default()
                    .push(candidate.id);
            }
        }

        let mut visited: HashSet<ObjectId> = HashSet::from([commit]);
        let mut frontier = vec![commit];

        while !frontier.is_empty() {
            let mut found: Vec<Reference> = Vec::new();
            let mut next = Vec::new();

            for id in frontier {
                let Some(children) = first_children.get(&id) else {
                    continue;
                };
                for &child in children {
                    if !visited.insert(child) {
                        continue;
                    }
                    let references = self.references_at(child);
                    if references.is_empty() {
                        next.push(child);
                    } else {
                        found.extend(references.iter().cloned());
                    }
                }
            }

            if !found.is_empty() {
                return found;
            }
            frontier = next;
        }

        Vec::new()
    }
}

/// What the detail panel shows for one commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDetail {
    pub commit: Commit,
    pub patch: Patch,
}

/// The filter row above the history table.
///
/// `query` is matched against subject, author name and abbreviated identifier. It filters
/// the loaded history in memory rather than re-reading, so typing stays responsive; `scope`
/// changes the walk and does trigger a re-read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryFilter {
    pub scope: HistoryScope,
    pub query: String,
}

impl Default for HistoryFilter {
    fn default() -> Self {
        Self {
            scope: HistoryScope::AllReferences,
            query: String::new(),
        }
    }
}

impl HistoryFilter {
    /// Whether `commit` survives the text query. An empty query keeps everything.
    pub fn matches(&self, commit: &CommitSummary) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let needle = self.query.to_lowercase();
        commit.summary.to_lowercase().contains(&needle)
            || commit.author.name.to_lowercase().contains(&needle)
            || commit.id.to_hex_prefix(40).starts_with(&needle)
    }
}

/// Emitted by the repository state so views reload only what actually changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoryEvent {
    /// `HEAD`, the reference list, or the loaded history changed.
    HistoryChanged,
    /// A different commit is selected, or its detail finished loading.
    SelectionChanged,
    /// A read failed. The message is already rendered for display.
    Failed(Arc<str>),
}

/// The sidebar's view of the repository, grouped as it is displayed.
#[derive(Clone, Debug, Default)]
pub struct ReferenceIndex {
    pub local_branches: Vec<RefEntry>,
    pub remote_branches: Vec<RefEntry>,
    pub tags: Vec<RefEntry>,
}

impl ReferenceIndex {
    pub fn from_entries(entries: Vec<RefEntry>) -> Self {
        let mut index = Self::default();
        for entry in entries {
            match entry.reference {
                Reference::LocalBranch(_) => index.local_branches.push(entry),
                Reference::RemoteBranch { .. } => index.remote_branches.push(entry),
                Reference::Tag(_) => index.tags.push(entry),
            }
        }
        index
    }

    pub fn fallback_branch(&self) -> Option<BranchName> {
        FALLBACK_BRANCHES.iter().find_map(|candidate| {
            self.local_branches
                .iter()
                .filter_map(|entry| match &entry.reference {
                    Reference::LocalBranch(branch) => Some(branch),
                    _ => None,
                })
                .find(|branch| branch.as_str() == *candidate)
                .cloned()
        })
    }
}

const FALLBACK_BRANCHES: [&str; 2] = ["main", "master"];

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Parents, Signature, TagName, Timestamp};

    fn id(nibble: char) -> ObjectId {
        nibble.to_string().repeat(40).parse().unwrap()
    }

    fn commit(summary: &str, author: &str) -> CommitSummary {
        CommitSummary {
            id: id('a'),
            parents: Parents::Root,
            summary: summary.to_string(),
            author: Signature {
                name: author.to_string(),
                email: String::new(),
                time: Timestamp {
                    seconds: 0,
                    offset_minutes: 0,
                },
            },
        }
    }

    #[test]
    fn an_empty_query_keeps_every_commit() {
        let filter = HistoryFilter::default();
        assert!(filter.matches(&commit("anything", "anyone")));
    }

    #[test]
    fn a_query_matches_the_subject_case_insensitively() {
        let filter = HistoryFilter {
            query: "REGISTER".into(),
            ..Default::default()
        };
        assert!(filter.matches(&commit("fix(register): make it run", "Someone")));
        assert!(!filter.matches(&commit("unrelated", "Someone")));
    }

    #[test]
    fn a_query_matches_the_author_name() {
        let filter = HistoryFilter {
            query: "parmantier".into(),
            ..Default::default()
        };
        assert!(filter.matches(&commit("anything", "Baptiste Parmantier")));
    }

    #[test]
    fn a_query_matches_an_identifier_prefix_but_not_a_suffix() {
        let filter = HistoryFilter {
            query: "aaa".into(),
            ..Default::default()
        };
        assert!(filter.matches(&commit("anything", "anyone")));

        let filter = HistoryFilter {
            query: "bbb".into(),
            ..Default::default()
        };
        assert!(!filter.matches(&commit("anything", "anyone")));
    }

    fn index_of(branches: &[&str]) -> ReferenceIndex {
        ReferenceIndex::from_entries(
            branches
                .iter()
                .map(|name| RefEntry {
                    reference: Reference::LocalBranch(BranchName::new(*name).unwrap()),
                    target: id('a'),
                    upstream: None,
                })
                .collect(),
        )
    }

    #[test]
    fn the_fallback_branch_is_main_when_it_exists() {
        assert_eq!(
            index_of(&["feature", "master", "main"]).fallback_branch(),
            Some(BranchName::new("main").unwrap()),
            "main wins over master wherever both exist, and whatever order they arrive in"
        );
    }

    #[test]
    fn the_fallback_branch_falls_back_to_master() {
        assert_eq!(
            index_of(&["feature", "master"]).fallback_branch(),
            Some(BranchName::new("master").unwrap())
        );
    }

    #[test]
    fn a_repository_with_neither_has_no_fallback_branch() {
        assert_eq!(
            index_of(&["trunk", "develop"]).fallback_branch(),
            None,
            "no fallback means deleting the checked-out branch is not offered at all, rather \
             than offered and failing with nowhere to go"
        );
    }

    #[test]
    fn references_at_returns_an_empty_slice_for_an_unreferenced_commit() {
        let history = History::default();
        assert!(history.references_at(id('a')).is_empty());
    }

    /// A trunk that merges one feature branch:
    ///
    /// ```text
    /// 9  main       merge, first parent d, second parent f
    /// |\
    /// | f  feature/x
    /// | |
    /// d e
    /// |/
    /// b
    /// |
    /// a
    /// ```
    ///
    /// Every node is named by a hexadecimal nibble because [`ObjectId`] rejects anything
    /// else — hence `9` where the diagram would rather say `m`.
    fn merged_feature_history() -> History {
        let commit = |nibble: char, parents: Parents| CommitSummary {
            id: id(nibble),
            parents,
            summary: String::new(),
            author: Signature {
                name: String::new(),
                email: String::new(),
                time: Timestamp {
                    seconds: 0,
                    offset_minutes: 0,
                },
            },
        };

        let commits = vec![
            commit('9', Parents::Merge(id('d'), id('f'))),
            commit('f', Parents::Linear(id('e'))),
            commit('d', Parents::Linear(id('b'))),
            commit('e', Parents::Linear(id('b'))),
            commit('b', Parents::Linear(id('a'))),
            commit('a', Parents::Root),
        ];

        let mut refs_by_commit = HashMap::new();
        refs_by_commit.insert(
            id('9'),
            vec![Reference::LocalBranch(BranchName::new("main").unwrap())],
        );
        refs_by_commit.insert(
            id('f'),
            vec![Reference::LocalBranch(
                BranchName::new("feature/x").unwrap(),
            )],
        );

        History {
            commits,
            refs_by_commit,
            ..Default::default()
        }
    }

    fn owning_names(history: &History, commit: char) -> Vec<String> {
        history
            .owning_references(id(commit))
            .iter()
            .map(Reference::short_name)
            .collect()
    }

    #[test]
    fn a_tip_owns_itself() {
        let history = merged_feature_history();
        assert_eq!(owning_names(&history, '9'), vec!["main".to_string()]);
        assert_eq!(owning_names(&history, 'f'), vec!["feature/x".to_string()]);
    }

    #[test]
    fn a_commit_written_on_a_merged_branch_is_owned_by_that_branch_alone() {
        let history = merged_feature_history();
        assert_eq!(
            owning_names(&history, 'e'),
            vec!["feature/x".to_string()],
            "main contains e through the merge, but only as a second parent — naming it \
             here would name every branch that ever absorbed the commit"
        );
    }

    #[test]
    fn a_commit_below_the_fork_is_owned_by_every_branch_that_forked_from_it() {
        let mut names = owning_names(&merged_feature_history(), 'b');
        names.sort();
        assert_eq!(names, vec!["feature/x".to_string(), "main".to_string()]);
    }

    #[test]
    fn a_commit_with_no_referenced_descendant_is_owned_by_nothing() {
        let mut history = merged_feature_history();
        history.refs_by_commit.clear();
        assert!(history.owning_references(id('e')).is_empty());
    }

    #[test]
    fn the_reference_index_splits_by_kind() {
        let entries = vec![
            RefEntry {
                reference: Reference::LocalBranch(BranchName::new("main").unwrap()),
                target: id('a'),
                upstream: None,
            },
            RefEntry {
                reference: Reference::RemoteBranch {
                    remote: domain::RemoteName::new("origin").unwrap(),
                    branch: BranchName::new("main").unwrap(),
                },
                target: id('a'),
                upstream: None,
            },
            RefEntry {
                reference: Reference::Tag(TagName::new("v1.0.0").unwrap()),
                target: id('b'),
                upstream: None,
            },
        ];
        let index = ReferenceIndex::from_entries(entries);
        assert_eq!(index.local_branches.len(), 1);
        assert_eq!(index.remote_branches.len(), 1);
        assert_eq!(index.tags.len(), 1);
    }

    #[test]
    fn a_reload_keeps_a_value_that_is_already_loaded() {
        let mut state: LoadState<u8> = LoadState::Ready(1);
        state.start_reload();
        assert_eq!(state, LoadState::Ready(1));
    }

    #[test]
    fn a_first_load_has_nothing_to_keep() {
        let mut state: LoadState<u8> = LoadState::Idle;
        state.start_reload();
        assert_eq!(state, LoadState::Loading);
    }

    #[test]
    fn a_reload_after_a_failure_has_nothing_to_keep() {
        let mut state: LoadState<u8> = LoadState::Failed("boom".into());
        state.start_reload();
        assert_eq!(state, LoadState::Loading);
    }

    #[test]
    fn a_reload_that_overtakes_another_stays_loading() {
        let mut state: LoadState<u8> = LoadState::Loading;
        state.start_reload();
        assert_eq!(state, LoadState::Loading);
    }

    #[test]
    fn load_state_exposes_only_the_matching_accessor() {
        let ready: LoadState<u8> = LoadState::Ready(1);
        assert_eq!(ready.ready(), Some(&1));
        assert!(!ready.is_loading());
        assert_eq!(ready.error(), None);

        let failed: LoadState<u8> = LoadState::Failed("boom".into());
        assert_eq!(failed.ready(), None);
        assert_eq!(failed.error(), Some("boom"));
    }
}
