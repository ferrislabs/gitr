# gitr

A Git client for macOS, written in Rust on [gpui](https://github.com/zed-industries/zed)
and [gpui-component](https://github.com/longbridge/gpui-component).

It replaces [GitX](https://github.com/rowanj/gitx), whose network operations crash on a
modal-sheet assertion and whose subprocess `git` inherits a truncated GUI `PATH`.

## Requirements

- macOS 15 or later
- Rust 1.97.1 (pinned by `rust-toolchain.toml`)

## Installing

```sh
cargo install --locked --git https://github.com/ferrislabs/gitr
```

`--locked` is not optional. gitr builds against `gpui` from zed's default branch, which
moves; `Cargo.lock` is what pins the revision it actually compiles with. `cargo install`
ignores a lock file unless told not to, resolves those git dependencies afresh, and fails
on whatever changed upstream since — most recently a `register_panel` signature that grew
two parameters.

A prebuilt Apple Silicon binary is attached to every [release](https://github.com/ferrislabs/gitr/releases).
It is unsigned, so macOS quarantines it on download and it needs
`xattr -d com.apple.quarantine gitr` before the first run — building it yourself avoids
that, since nothing is downloaded.

That installs one command, `gitr`:

```sh
gitr              # open the repository containing the working directory
gitr ~/code/foo   # open a repository by path
```

Run from a directory that is not inside a repository, `gitr` reopens the projects it
already knows. Naming a path that is not a repository is an error, because you asked for
that one by name.

The command returns the prompt straight away and leaves the window running, so the
terminal stays yours, and the window comes to the front as it opens. Errors still land in it: the repository is resolved before the app
detaches, so `gitr /nonexistent` fails where you can see it and starts nothing.
`GITR_FOREGROUND=1 gitr` keeps everything in one attached process, which is the only way
to see a panic or a log line.

gitr is not on crates.io, and cannot be for now. `gpui` is reachable only from git, and
zed's February 2026 split left the version on crates.io frozen and unable to receive the
rest of the framework. `CLAUDE.md` records what changing that would cost.

## Running from a checkout

```sh
cargo run -p gitr_gui
```

The first build compiles roughly 850 crates and takes several minutes. Subsequent
incremental builds are a couple of seconds.

## Layout

| Directory | Package | Role |
|---|---|---|
| `crates/domain` | `gitr-domain` | Entities, value objects, ports. No infrastructure, no gpui. |
| `crates/graph` | `gitr-graph` | Commit graph lane layout. Pure, no I/O. |
| `crates/vcs` | `gitr-vcs` | Git adapters: `gix` for reads, subprocess `git` for patches and mutations. |
| `crates/ui` | `gitr-ui` | Views built on gpui-component. |
| `crates/gitr` | `gitr_gui` | Binary `gitr`, and composition root. |

The packages carry a `gitr-` prefix because crates.io has a single global namespace in
which the short names were taken. The code does not: each library pins its `[lib] name`
back to the short form, so imports read `use domain::…`. Cargo's `-p` flag names packages,
not libraries, so it takes the prefixed form.

`gitr-domain` and `gitr-graph` do not depend on gpui, so `cargo test -p gitr-domain -p
gitr-graph` runs in seconds rather than minutes.

## Contributing

[`CONTRIBUTING.md`](CONTRIBUTING.md) covers the build, the test loop, and the
architectural rule a change has to respect.

## License

[Apache 2.0](LICENSE).

Releases up to and including the last MIT-licensed commit stay available under
MIT — a licence already granted cannot be withdrawn. Apache 2.0 applies from
this commit forward.
