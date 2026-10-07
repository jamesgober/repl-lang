# repl-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../../_strategy/LANG_COLLECTION.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt.

## v0.2.0 - Core (THE HARD PART, NOT DEFERRED) (DONE)
REPL scaffolding: line editing, session state, incremental feed into the pipeline.
Dependencies (wires lexer, parser, diag) are wired here, when first used.
Exit criteria:
- [x] Every public item has rustdoc + a runnable example.
- [x] Core invariants property-tested (full DIRECTIVES + API authored at this stage).

Delivered 2026-07-08: `Session`, `Input`, `Status`, `Feed`, `Entry`,
`SessionError`, `Editor`, `Edit`. Incremental feed re-runs the caller's
pipeline on the whole pending entry after each line; the pipeline's own parse
errors decide completeness (`Input::is_incomplete`: every error at the end of
input means "keep reading"). Committed entries live in one `SourceMap` as
`<repl:N>`, and the pipeline learns its entry's global base before commit, so
spans are final when made and diagnostics can reach into earlier entries. The
editor is terminal-agnostic: hosts map keys to `Edit` commands. Scaffold
defects fixed on the way in: unquoted `keywords` / `categories` in
`Cargo.toml` (the manifest did not parse), clippy MSRV `1.87` → `1.85`, BOMs in
docs, a stale project name in `deny.toml`, a dead `dev/DIRECTIVES.md` link.

Dependency wiring, recorded under the anti-deferral rule:

- **`lexer-lang` — wired.** `Input::cursor` returns a `Cursor` positioned at
  the entry's global base, so token spans need no rebasing.
- **`diag-lang` — wired.** `Input::is_incomplete` reads `Diagnostic`s; default
  features (ANSI colour) are left for the application to choose.
- **`source-lang` — wired** (not in the original list). Committed entries are
  sources in its `SourceMap`, which is what diag-lang renders against.
- **`unicode-lang` — wired** (not in the original list). The editor needs
  display widths for the cursor column and `XID_Continue` for word motion; the
  crate has no dependencies and is already the family's Unicode source.
- **`parser-lang` — not wired, by design.** The integration point between a
  REPL and a parser is the parser's diagnostics, which are `diag-lang` types;
  nothing in the session's API names a parser type, so a normal dependency
  would add weight without adding surface. Every parser that reports
  end-of-input errors at the end — parser-lang included — works through
  `Input::is_incomplete`. parser-lang is a dev-dependency: the examples and the
  integration tests drive a real parser-lang grammar through the session. This
  is a design decision, not a deferral.
- **`serde` — removed.** The scaffold declared it with no code behind it.
  Persisting history is already possible through `Editor::history` and
  `Editor::add_history`; a serde feature can be added later if a use appears.

## v1.0.0 - API freeze (DONE)
Public surface stable and frozen until 2.0.
- [x] docs/API.md marked stable; SemVer promise recorded.
- [x] Full test + benchmark suite green on all three platforms.

Shipped 2026-07-08. The freeze review changed no code: `Session`, `Editor`,
`Input`, and `Entry` are opaque, so they can grow methods freely; `Edit` and
`SessionError` were `#[non_exhaustive]` from 0.2.0; and `Status` and `Feed` stay
exhaustive on purpose, because hosts match `Feed` in every read loop and the
set (finished, unfinished, nothing to evaluate) is closed by design. The
surface, the entry layout and feeding rules, the `is_incomplete` rule, the
default limits, the editing rules, and MSRV 1.85 are recorded as the contract
in `docs/API.md#stability`. Tests and benchmarks green on Windows and Linux
(WSL2) locally; macOS through the CI matrix.

Additive 1.x candidates (not commitments): a configurable entry-name prefix
(`<calc:N>` instead of `<repl:N>`), an editor method to replace a range of the
line for completion without touching the kill buffer, prefix-filtered history
search, and a `serde` feature for history persistence.
