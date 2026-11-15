# Aster

Native GPU-accelerated terminal in Rust. Rendering: D3D11 + DirectComposition + DirectWrite on Windows, OpenGL ES 3 + EGL + HarfRust + Swash on Linux.

## Docs (RU) — read before coding

- `doc/` — private working docs, git-ignored, never pushed: `doc/PLAN.md` (stages, tasks, decisions, status), `doc/FEATURES.md` (feature checklist with config keys), `doc/tasks/` (task briefs for helper agents). Tick features in `doc/FEATURES.md` when they ship. Agent work products stay in `doc/`.
- `docs/` — public docs that go to the repository: `docs/releases/X.Y.Z.md` (release notes, read by CI). Add something here only when it is meant for users.
- `ARCHITECTURE.md` (crates, threads, input → PTY → frame path, capture-based visual checks) — always read.

The project is self-contained: docs, code, UI strings, tests and commit messages never mention other applications or terminals. Third-party code enters only as crates.io dependencies or bundled fonts with their licenses.

## Code

- Never write code comments. Zero comments anywhere in this repository: no `//`, `///`, `//!`, `/* */` in Rust, HLSL or WGSL, no `#` comments in TOML/YAML/PowerShell. Names and structure carry the meaning; prose belongs in README.md, ARCHITECTURE.md and docs/.
- `aster-gfx`, `aster-vt`, `aster-core`, `aster-platform` (and later `aster-proto`, `aster-vault`) never depend on the `windows` crate (windows-rs), directly or transitively; `windows-sys` pulled by `alacritty_terminal` for its unused tty module is the only allowed exception. OS code lives only in platform crates under `#![cfg(...)]`. CI job `deps-isolation` enforces it.
- The core builds a `DrawList`, the platform draws it. No OS calls from core crates; effects go out through queues.
- UI thread never blocks on I/O or on the PTY. Reading the PTY, parsing and writing happen on worker threads.
- Redraw only on change.

## Commits

One English sentence, no body, no trailers, no `Co-Authored-By`. Commit and push to `master` without asking.

## Commands

Builds use at most 8 parallel jobs (`.cargo/config.toml`, `-j 8` for nextest).

```bash
cargo check -p <crate>
cargo nextest run -j 8 -p <crate>
cargo nextest run -j 8 --workspace
cargo clippy --workspace --all-targets -j 8 -- -D warnings
cargo run -p aster
```

## Language

English: code, commits, UI strings, CLAUDE.md. Russian: `docs/`, ARCHITECTURE.md, README.md, reports.
