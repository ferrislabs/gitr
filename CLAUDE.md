# CLAUDE.md

Guidance for Claude Code working in this repository.

gitr is a macOS Git client built on gpui and gpui-component. It replaces GitX.

## Where things live

| Path | Owns |
|---|---|
| `crates/domain/` | Entities, value objects, ports. Depends on nothing but `thiserror`. |
| `crates/graph/` | Commit graph lane layout. Pure function over `domain` types, no I/O. |
| `crates/vcs/` | Git adapters. `gix` for structured reads, subprocess `git` for patches. |
| `crates/ui/` | Views on gpui-component. |
| `crates/gitr/` | Binary. Composition root — the only place adapters are wired to ports. |

Dependencies point inward. `domain` and `graph` must never depend on `gix`, on a
subprocess, or on gpui. That is not style: it is what keeps `cargo test -p domain -p graph`
at a few seconds instead of a few minutes.

## Commands that actually run

```sh
cargo test -p gitr-domain -p gitr-graph   # fast loop, no gpui in the tree
cargo test --workspace                    # exit check
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p gitr_gui                     # launch the app
```

`-p` takes the *published package* name, so it is prefixed even though the code imports these
crates unprefixed — `-p domain` and `-p gitr` both fail with "did not match any packages".

Cargo holds one lock per target directory, so `clippy` and `test` do not run
concurrently in the same checkout. Batch them only across distinct `CARGO_TARGET_DIR`s.

## Non-obvious constraints

**Do not pin `gpui` to a `rev`.** gpui-component declares `gpui` against the default
branch of `zed-industries/zed`, and Cargo unifies two git sources only when the
reference matches exactly. A `rev` here produces two copies of the crate in the graph,
and traits from one do not apply to the other. The failure looks unrelated — *"no method
named `bg` found for struct `Root`"*. `Cargo.lock` is what pins the revisions.

**Because `Cargo.lock` is the only pin, every install command needs `--locked`.**
`cargo install` ignores a lock file by default, re-resolves `gpui` and `gpui-component`
against zed's moving default branch, and fails on whatever changed upstream since the
lock was written — the last time, a `register_panel` closure that grew from three
parameters to five. The build is green in a checkout and broken for anyone installing,
which is the worst shape a break can take: `cargo build` here will never reproduce it.
`cargo install --locked --git …` is the command; the README, `CLAUDE.md` and the release
notes in `.github/workflows/release.yaml` must all carry the flag.

**The gpui-component website documentation is out of date in places.** Verified wrong at
the time of writing: `h_resizable(id, state)` is really `h_resizable(id).with_state(&state)`;
`Root::render_dialog_layer(cx)` really takes `(window, cx)`; `Root::new(...).bg(...)` does not
exist; the `popup_menu` module is not public — it is `gpui_component::menu`. Read
`crates/ui/src/` and `crates/story/src/stories/` in the gpui-component checkout instead.

**`Root` must be the first child of the window**, or dialogs, sheets and notifications break.

**`theme::init` forces light mode** at startup. Following the OS appearance needs an explicit
`window.observe_window_appearance(..)` subscription plus one
`Theme::sync_system_appearance(Some(window), cx)` at launch.

**Without `.with_assets(gpui_component_assets::Assets)` every icon renders blank**, including
the window controls `TitleBar` draws. gpui logs and no-ops on a missing asset, so nothing
fails — the icons are simply invisible. Note the bundled set contains **no VCS icons at all**:
no branch, tag, remote, stash, commit or diff glyph.

**The dock icon cannot come from a bundle, because there is no bundle.** gitr is installed
as a bare binary, so macOS has no `Info.plist` to read `CFBundleIconFile` from and the dock
shows a blank executable. gpui offers nothing here either — its platform trait stops at
`set_dock_menu`. `crates/gitr/src/main.rs` therefore calls AppKit directly:
`NSApplication::setApplicationIconImage` with an `NSImage` decoded from an embedded PNG.
It must run on the main thread and after gpui has created the `NSApplication`, which is
what the `run` closure guarantees.

**The icon is pixel art, and pixel art is placed rather than sampled.** The mark is an
11×11 grid enlarged by whole pixels — there is no SVG and nothing is rasterised. Feeding
the old vector artwork to a rasteriser at icon size does not convert it, it produces mush,
because a curve sampled onto a coarse grid lands wherever it lands. The crab itself is not
a redrawing either: it is lifted pixel for pixel from `github.com/ferrislabs.png`, which is
a 9×9 grid in three colours, and that economy is why it survives at dock size. Larger
hand-drawn crabs were tried and each one read as a face. One pixel departs from the
original — the whites of the eyes are transparent there, sitting on GitHub's white plate,
and must be set explicitly here because this plate is black.

**A macOS icon does not fill its canvas, and one that does looks the wrong size.** Measured
on Mail, Messages, Music and Slack, each puts its artwork on 824 of 1024 pixels — 80.5% —
and leaves the rest transparent. The dock lays out the canvas, not the artwork, so painting
edge to edge puts the icon on a different grid from every neighbour. gitr ships those exact
numbers.

**The plate is antialiased even though the crab is not.** Quantising the plate's corner to
the crab's grid was tried and looks broken in the dock: every icon beside it has a smooth
silhouette, so a stepped corner reads as a rendering fault rather than as a style. The
plate is a superellipse of exponent 5 — Apple's silhouette runs its curvature continuously
into the straight edge instead of meeting it at a tangent, which a rounded rectangle cannot
do — sampled by sub-row rather than sub-pixel, since its half-width is analytic for any y.

Flat colour also compresses: the render is 11 KB, where the gradient-and-grain version it
replaced needed 231 KB at half the resolution.

**`TitleBar::window_options()`, not `WindowOptions::default()`.** The title bar draws its own
chrome and needs the OS one transparent and drag-owned by the app.

**Further API-versus-documentation gaps found the hard way:**

- `Context<T>::on_app_quit` (two arguments) shadows `App::on_app_quit` (one) when called on a
  `&mut Context<T>`. The inherent method wins.
- `SidebarHeader::dropdown_menu(..)` returns `DropdownMenuPopover<SidebarHeader>`, not
  `SidebarHeader`.
- `SidebarHeader` and `SidebarFooter` do not implement `FluentBuilder`, so there is no
  `.when(..)` on them, unlike `SidebarMenuItem` and `Div`.
- `dock::DockState` has private fields despite being `pub` and `Serialize`, and can only be
  built from a live window. `DockAreaState`, `PanelState` and `PanelInfo` are fully public, so
  a pure persistence test can only exercise `center`.
- `ThemeColor::light()` and `ThemeColor::dark()` work outside any `App`, which makes theme
  mapping unit-testable. `ThemeColor::default()` does not — every field is zeroed.
- A panel referenced by a saved layout whose name was never passed to `PanelRegistry` falls
  back silently to `InvalidPanel`. Register panels before loading a layout.

**Never call `TableState::dump`** — it materialises the whole table (gpui-component
issue #2754). On a large history that is an out-of-memory crash.

**`TableDelegate::visible_rows_changed` runs every scroll frame.** Keep it allocation-free.

**History is walked in date order, matching GitX.** `gix_traverse::commit::topo::Builder`
with `Sorting::DateOrder` reproduces `git rev-list --date-order` exactly — verified by
diffing the two sequences over this repository. That builder is the way in either case: it
is not exposed on gix's high-level `rev_walk`.

The order was topological until the graph was made to match GitX. An older measurement
argued for it — `rust-lang/cargo` at 23 789 commits, 258 lanes in date order against 20
topologically — and it was taken before the walk seeded from every reference, so it does
not describe this code. Measured on the current code, `zed-industries/zed` at 39 565
commits gives **13 columns in date order against 17 topologically**: date order is the
narrower of the two here. Do not restore the old ordering on the strength of the old
number; re-measure if it comes up.

**Lane placement is a port of GitX's `PBGitGrapher.decorateCommit`, not an adaptation.**
Columns compact — a row's are rebuilt by walking the previous row's and appending the
survivors, so a column is a position among them and everything right of an ending track
slides left. Giving each track a column it keeps was tried and reverted: easier to follow,
but not what GitX draws.

Two details carry the whole difference, and both were got wrong before being ported
faithfully. **A commit's node sits at its index in the outgoing column list, not the
incoming one** — they agree until a column dies in the same row a tip appears. And
**convergence is deferred**: a first parent takes over its lane without checking whether
another column already expects it, so two columns hold the same object until the row that
places it. Converging eagerly closes a column a row early and shifts everything right of
it. Verify any change here by running GitX's algorithm over a real history and diffing the
column of every commit — ten of this repository's 53 were off by one before the port, and
patching rather than porting took it to eighteen.

**A graph line bends nowhere.** `PBGitRevisionCell.drawLineFromColumn` draws one straight
line from a cell edge to that cell's own centre, so a change of column happens over half a
row and every line ends *on* a node. `GraphRow` is split to match — `incoming` for the
upper half, `segments` for the lower — and correct columns are not enough on their own: a
segment spanning the whole band between two rows draws the right topology with every
divergence starting a half-row below the node it comes from.

**Every half-extent in the gutter must be a whole number of pixels.** A commit's node is a
disc in the track's colour with a smaller one over it, and lines run through both. Each
shape — quad or stroked path — is snapped to the device grid on its own, and what gets
snapped is a *half-extent*: a radius for a disc, half a width for a line. Two shapes on the
same centre therefore stay centred together only when their half-extents share a fractional
part, and whole numbers is the only value that satisfies every pair at once.

Two bugs came from breaking that, and both read as drawing errors rather than as rounding.
A 1.2px ring put the two discs' origins on different offsets, so the fill sat half a pixel
off — on screen, a ring 2px thick on one side and 1px on the other. Then a radius of 4.5
against a half-line-width of 1.0 put every vertical line half a pixel off the node it ran
through. `crates/ui/src/history/geometry.rs` asserts the rule over all three half-extents.

The corollary is that a line's width is not free either: it must be even. That is why the
gutter uses GitX's own `setLineWidth:2` rather than the 1.5 it started with.

**Network and mutating Git operations go through subprocess `git`, never a library.**
libssh2 does not read `~/.ssh/config`, so `Host` aliases and `ProxyCommand` silently break.
The subprocess must inherit a `PATH` resolved from the user's login shell — a macOS GUI
app gets a truncated one, which is why GitX fails with `git: 'credential-osxkeychain' is
not a git command`.

The same limitation reaches *installing* gitr, because cargo fetches through its own
bundled libgit2 too. It does not bite today — this repository is public, so
`cargo install --locked --git https://github.com/ferrislabs/gitr` fetches anonymously with
no credentials, verified with `GIT_CONFIG_GLOBAL=/dev/null`. It would return the
moment the repository went private again: HTTPS then fails for want of a credential
helper, SSH fails with `no authentication methods succeeded` because libgit2 reads neither
`~/.ssh/config` nor the agent, and only `--config net.git-fetch-with-cli=true` — which
hands the fetch to the `git` binary — gets through.

**`AsyncApp` is not `Send`.** It holds `Weak<AppCell>` and an `Rc`, so nothing off the main
thread can call into gpui through it. A background thread reaching the app — the
single-instance listener in `crates/gitr/src/instance.rs` is the case in the tree — hands
its result over an `async_channel` whose receiver is awaited inside `cx.spawn`. Reaching
for `AsyncApp` in the thread instead does not compile, but it is the obvious first design
and worth not attempting twice.

**One gitr process, whatever the number of open repositories.** A second `gitr <path>`
offers the resolved root to a Unix socket in the application support directory and exits;
the live instance opens another window. Two things that look optional are not: a socket
file outlives the process that made it, so `bind` must probe with a connect and unlink a
stale one rather than trusting the file's absence, and two invocations racing both reach
the bind, so the loser must hand its path to the winner instead of reporting a failure the
user cannot act on.

**No blocking modal for long operations.** GitX crashes on
`assert(currentModalSheet == nil)` when two network operations overlap. Long work runs on
`cx.background_executor()` and reports through the status bar and notifications.

**Canonicalise every path before handing it to `notify`.** FSEvents reports canonical paths,
and on macOS `/var` is a symlink to `/private/var`. A repository reached through a symlinked
path makes notify's own filter drop every event, silently, while the watcher still looks
alive.

**`.git` is a file, not a directory, in a linked worktree and in a submodule.** It holds
`gitdir: <path>`. Resolve it before watching or reading, or the operation observes nothing
and reports no error.

**A plain `git commit` does not rewrite `.git/HEAD`.** Only `refs/heads/<branch>` changes;
`HEAD` keeps the same symbolic-ref text and its mtime does not move. Detecting a commit by
watching `HEAD` alone misses every commit made on an attached branch.

**`git show` and `git diff-tree -p` print nothing for a merge commit**, and an unknown object
and a valid root commit produce byte-identical stderr. `crates/vcs/src/process/` diffs a merge
against its first parent and detects the root commit with two `rev-parse --verify` probes
rather than by matching git's error text.

**A crate's published name, its lib target and its import name are three separate things**,
and this workspace uses all three. crates.io has one flat global namespace in which `gitr`,
`domain`, `ui` and `graph` were already taken, so the packages are `gitr_gui`, `gitr-domain`,
`gitr-graph`, `gitr-vcs` and `gitr-ui`. Each library then pins `[lib] name` back to the short
form and the root `Cargo.toml` renames the dependency (`domain = { package = "gitr-domain",
… }`), so every `use domain::…` in the source keeps working untouched. Cargo's own `-p` flag
does not: it names packages, so it takes the prefixed form. The binary is
`[[bin]] name = "gitr"` inside package `gitr_gui`, so `cargo install gitr_gui` installs a
command called `gitr`.

**Renaming a lib target breaks `tests/` without breaking `src/`.** A crate's own integration
tests reach it by its *lib target* name, so dropping `[lib] name` here would leave
`cargo check --workspace` green and fail only under `cargo test --workspace`. Path deps also
need a `version` alongside `path` or nothing publishes.

**gitr cannot be published to crates.io while `gpui` comes from git.** `cargo publish` rejects
it outright: *"all dependencies must have a version requirement specified when publishing"*.
Adding `version` next to `git` silences that, but it publishes a crate compiled against zed's
default branch while telling users to build it against crates.io `gpui 0.2.2` — untested by
construction. `gpui_platform` is not on crates.io at all. Verify with
`cargo publish --dry-run -p <crate> --allow-dirty`; `gitr-domain` passes today because it
depends only on `thiserror`. Publication order follows the dependency graph: `gitr-domain`,
then `gitr-graph` and `gitr-vcs`, then `gitr-ui`, then `gitr_gui`.

## House rules

- No `async_trait`, ever. gpui has its own executor; ports are synchronous and blocking,
  called from `cx.background_executor().spawn()`. tokio is not in the tree.
- Prefer enum dispatch to `Box<dyn Trait>` wherever the set of implementations is closed.
- Crates are imported unprefixed — `use domain::…`, never `use gitr_domain::…`. The
  `gitr-` prefix exists only in the published package name, for crates.io's namespace.
- Commit convention: `<type>(<subject>): <imperative message>`.
- Never add an AI co-author trailer to a commit, issue or pull request.
