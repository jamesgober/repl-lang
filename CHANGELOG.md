<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>repl-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [1.0.0] - 2026-07-08

API freeze. The public surface introduced in 0.2.0 is now stable and frozen
under Semantic Versioning: no breaking changes ship before `2.0`. The freeze
review found nothing to change in the code — every type users construct or
match was already either opaque or deliberately closed — so this release
records the contract.

### Changed

- Bumped the crate version to `1.0.0` and declared the public API stable.
- `docs/API.md` marked stable with a recorded SemVer promise: the frozen
  surface, and the behaviour that belongs to it — entry naming and layout in
  the session's source map, line-terminator normalisation and blank-line
  handling, the exact `Input::is_incomplete` rule, the default entry limit and
  history size, the display-unit, word, history, and draft rules of the editor,
  and MSRV 1.85 as a compatibility surface. `Status` and `Feed` are documented
  as deliberately exhaustive; `Edit` and `SessionError` remain
  `#[non_exhaustive]` so commands and error cases can be added in `1.x`.
- Crate-level documentation gained a Stability section; the README and
  `docs/API.md` install snippets now use `repl-lang = "1"`, and the README
  performance table reports Windows and Linux side by side.

---

## [0.2.0] - 2026-07-08

The core, and the hard part of the roadmap: the scaffold becomes working REPL
scaffolding. A session decides when typed input forms a complete entry by
asking the language's own pipeline, keeps every entry in one source map so
diagnostics can point anywhere in the session, and a terminal-agnostic line
editor handles the line being typed. The surface is small: two engines
(`Session`, `Editor`), the values they exchange, and one error type.

### Added

- `Session` — accumulates lines into entries and runs a caller-supplied
  pipeline closure on the whole entry after each line (`feed`). Blank lines
  with nothing pending return without calling the pipeline; a single trailing
  `"\n"` or `"\r\n"` is normalised, so `read_line` output and `str::lines`
  items behave the same. Completed entries are committed to a `SourceMap` as
  `<repl:N>` (`sources`, `entry`, `len`, `is_empty`); `cancel`, `is_pending`,
  and `pending` support interrupts and prompts. `with_limit` (default 1 MiB)
  bounds the size of one entry.
- `Input` — the pipeline's view of the pending entry: its text, its global
  `base` and `span` in the session's source map (known before the entry is
  committed, so every span the pipeline records is final), its entry `number`,
  a `lexer_lang::Cursor` already positioned at the base (`cursor`), and
  `is_incomplete`, which recognises parser errors that all sit at the end of
  input as "keep reading" rather than "wrong".
- `Status` (`Complete(T)` / `Incomplete`) — the pipeline's verdict, carrying
  its result so a finished entry is parsed once.
- `Feed` (`Complete { entry, value }` / `Incomplete` / `Empty`) and `Entry`
  (`number`, `id`, `span`) — the session's answer to one line.
- `SessionError` — `#[non_exhaustive]`; `TooLong` and `SpaceExhausted`. The
  pending entry is discarded and the pipeline is not called, so the session
  stays usable. Errors report sizes, never the refused text.
- `Editor` and `Edit` — a single-line editor with no terminal dependency:
  insertion (control characters refused), deletion and cursor motion by display
  unit (a visible character plus attached combining marks), word motion over
  `XID_Continue` runs, kill and yank, a bounded history that preserves the
  half-typed line while browsing (`submit`, `add_history`, `history`), and the
  cursor's terminal column (`column`) with wide characters counted as two.
  Buffers and history slots are recycled, so steady-state editing does not
  allocate.
- Re-exports of the family types that appear in signatures: `Cursor`,
  `Diagnostic`, `SourceMap`, `SourceFile`, `SourceId`, `Span`, `BytePos`.
- Three runnable examples (`calc`, `notebook`, `line_editing`), two of them
  sharing a calculator front end built on `lexer-lang` and `parser-lang`;
  Criterion benchmarks; integration tests; property tests checking the editor
  against a `Vec<char>` reference model and the session against a string
  accumulator; and full `docs/API.md`.

### Changed

- Bumped the crate version to `0.2.0`.
- Wired `lexer-lang` 1 (`Input::cursor`), `diag-lang` 1 (`Input::is_incomplete`
  reads its diagnostics; default features are left to the application), and
  `source-lang` 1 (the session's source map). Added `unicode-lang` 1 for
  display widths and word classes in the editor. `parser-lang` is used by the
  examples and tests but is deliberately not a library dependency; the
  reasoning is recorded in `dev/ROADMAP.md`.
- Removed the unused `serde` feature and dependency and the unused `loom`
  dev-dependency. Neither had code behind it.
- Applied the full REPS crate-root lint set in `src/lib.rs`.

### Fixed

- Corrected invalid TOML in `Cargo.toml`: `keywords` and `categories` were
  unquoted, so the manifest did not parse at all.
- Aligned the `clippy.toml` MSRV (`1.87` → `1.85`) with `Cargo.toml`.
- Removed UTF-8 byte-order marks from `docs/API.md` and `dev/ROADMAP.md`, a
  stale project name in `deny.toml`, and a missing final newline in
  `rustfmt.toml`.
- README contributing guidance now points at `REPS.md`; it previously linked a
  `dev/DIRECTIVES.md` that does not exist.

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/repl-lang/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jamesgober/repl-lang/compare/v0.2.0...v1.0.0
[0.2.0]: https://github.com/jamesgober/repl-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/repl-lang/releases/tag/v0.1.0
