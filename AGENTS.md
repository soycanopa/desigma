# AGENTS.md

Working rules for coding agents in this repository. The source of truth is
[CONTRIBUTING.md](CONTRIBUTING.md); if the two ever disagree, CONTRIBUTING.md wins.

## Build and run

```sh
cargo run -p desigma-app               # dev builds are usable (deps build at opt-level 2)
cargo run --release -p desigma-app     # the editor with the demo document
cargo run -p desigma-app -- file.fig   # open a Figma file
```

The toolchain is pinned by `rust-toolchain.toml`. Linux needs the windowing headers
listed in CONTRIBUTING.md.

## Before opening a PR, run these

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings   # blocking: must be warning-free
cargo fmt --all --check                                  # advisory (the tree predates rustfmt.toml)
```

- Clippy is blocking. If a lint is genuinely wrong for a spot, add a narrowly scoped
  `#[allow(...)]` with a one-line reason instead of restructuring.
- Format only the files your change already touches; no drive-by reformatting.
- CI (`.github/workflows/ci.yml`) rebuilds and retests on Linux, Windows and macOS with
  `--locked` and `RUSTFLAGS: -D warnings`, plus a blocking clippy job and a non-blocking
  fmt job. Keep `Cargo.lock` in sync with `Cargo.toml`.

## Verifying UI changes

Render one frame headlessly and diff it:

```sh
desigma --screenshot out.png --size 1600x1000 --panel variables-floating
desigma-diff compare before.png after.png --out diff.png --tolerance 2 --threshold 0.001
```

- Editor changes use approved desigma screenshots as the regression baseline.
- Never commit the Figma UI captures under `reference/screens/` (local-only, git-ignored)
  and never use them as an editor acceptance target.
- Rendering changes must stay deterministic; compare canvas-only output from permitted
  test documents at matching zoom, dimensions and font settings.

## Commits and tests

- Short imperative subject with an area prefix (`app: assets panel thumbnails`,
  `doc: variables with modes`, `render: cached shadows`). Explain the why in the body
  when it is not obvious.
- Keep commits logically separate: a lint pass, a feature and a docs change are three commits.
- Add or update tests with every behaviour change. Editor behaviour is tested end-to-end
  with the `Sim` harness in `crates/desigma-app/src/shell/tests.rs`: it drives the whole
  window with synthetic egui input, and widgets are found by name via `Sim::rect_of`
  after `widgets::record`. New interactive UI records itself there too.
- Screenshot runs can transiently fail with "screenshot was not captured" (a window
  sizing race); rerun before investigating.

## Where code goes

- `desigma-app` is the only UI crate. Dependencies flow downward: `app` depends on
  everything, `doc` depends on nothing in the workspace. Keep UI code out of the lower
  crates and make document edits go through undoable `Command`s.
- Local caveat: `sf_pro_text_renders_through_inter` (desigma-render) fails on macOS
  machines that can resolve SF Pro Text locally; it passes on the CI runners. Verify with
  `git stash` before blaming an editor change.

## Assets

Test documents and third-party assets need licenses that permit the intended use and
distribution, with required attribution preserved.
