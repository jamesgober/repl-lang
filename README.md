<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>repl-lang</b>
    <br>
    <sub><sup>REPL SCAFFOLDING</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/repl-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/repl-lang"></a>
    <a href="https://crates.io/crates/repl-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/repl-lang?color=%230099ff"></a>
    <a href="https://docs.rs/repl-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/repl-lang"></a>
    <a href="https://github.com/jamesgober/repl-lang/actions"><img alt="CI" src="https://github.com/jamesgober/repl-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>repl-lang</strong> is the loop state of a read-eval-print loop, with the language and the terminal left out. It decides when typed input forms a complete entry, keeps every entry of the session so diagnostics can point back into them, and edits the line being typed &mdash; the parts every language REPL needs and none should have to write twice.
    </p>
    <p>
        The hard question in a REPL is <em>is the user finished?</em> After <code>let area = width * (3 +</code> the answer is plainly no, and the prompt should continue rather than report a syntax error. repl-lang answers it with the language's own parser: a <a href="./docs/API.md#session"><code>Session</code></a> runs your pipeline on the entry after every line, and <a href="./docs/API.md#inputis_incomplete"><code>Input::is_incomplete</code></a> recognises the case where every parse error sits at the very end of the input &mdash; the parser ran out of text, so ask for another line. Each finished entry becomes a source named <code>&lt;repl:N&gt;</code> in one shared source map, so an error in entry 7 can underline a definition from entry 2. A terminal-agnostic <a href="./docs/API.md#editor"><code>Editor</code></a> handles cursor motion, word motion, kill and yank, and history, driven by commands your key handler produces.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, built on <a href="https://crates.io/crates/lexer-lang"><code>lexer-lang</code></a>, <a href="https://crates.io/crates/diag-lang"><code>diag-lang</code></a>, <a href="https://crates.io/crates/source-lang"><code>source-lang</code></a>, and <a href="https://crates.io/crates/unicode-lang"><code>unicode-lang</code></a>.
    </p>
    <blockquote>
        <strong>1.0.0 is the API freeze.</strong> The public surface is stable and follows Semantic Versioning &mdash; no breaking changes before <code>2.0</code>. See <a href="./docs/API.md#stability"><code>docs/API.md</code></a> for the frozen-surface list and the SemVer promise, and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## The model

Two independent halves, usable together or apart:

- **Session state.** A **[`Session`](./docs/API.md#session)** accumulates lines into an entry and calls your pipeline — a closure that lexes, parses, and evaluates — with an **[`Input`](./docs/API.md#input)** over the whole entry after each line. The pipeline answers **[`Status`](./docs/API.md#status)**`::Complete(value)` or `Status::Incomplete`; the session answers with a **[`Feed`](./docs/API.md#feed)**, and on completion commits the entry and hands back an **[`Entry`](./docs/API.md#entry)** with the value.
- **Line editing.** An **[`Editor`](./docs/API.md#editor)** holds the line, the cursor, a kill buffer, and a bounded history. Your key handler maps keys to **[`Edit`](./docs/API.md#edit)** commands; the editor applies them and reports whether to redraw.

<br>

What each part owns, and what it leaves to you:

| | repl-lang | Your code |
|---|---|---|
| **Completeness** | Re-running the pipeline per line; `is_incomplete` over parser errors | The parser |
| **Positions** | One global span space across all entries; `Input::cursor` starts at the right offset | Tokens and syntax built from those spans |
| **Diagnostics** | Every entry stored as `<repl:N>` for rendering | Deciding what is wrong |
| **Editing** | Line, cursor, word motion, kill/yank, history | Reading keys, drawing the screen |
| **State** | Entry numbering, pending text, size limit | Interpreter state, captured by the pipeline closure |

<hr>
<br>

## Installation

```toml
[dependencies]
repl-lang = "1"
diag-lang = "1"     # rendering diagnostics against Session::sources
```

Or from the terminal:

```bash
cargo add repl-lang diag-lang
```

MSRV: Rust 1.85 (Rust 2024 edition).

<hr>
<br>

## Quick start

A session over a fixed script with a pipeline that keeps reading until parentheses balance. In a real REPL the lines come from a terminal and the pipeline is your parser.

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let mut transcript = Vec::new();

for line in ["(define (square x)", "  (* x x))", "", "(square 12)"] {
    let prompt = if session.is_pending() { "..." } else { ">>>" };
    let feed = session.feed(line, |input| {
        let depth: i32 = input.text().chars().map(|c| match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        }).sum();
        if depth > 0 { Status::Incomplete } else { Status::Complete(input.text().lines().count()) }
    })?;
    match feed {
        Feed::Complete { entry, value } => {
            transcript.push(format!("{prompt} {line}   [entry {} spans {value} lines]", entry.number()));
        }
        Feed::Incomplete => transcript.push(format!("{prompt} {line}")),
        Feed::Empty => transcript.push(prompt.to_owned()),
    }
}

assert_eq!(transcript, [
    ">>> (define (square x)",
    "...   (* x x))   [entry 1 spans 2 lines]",
    ">>>",
    ">>> (square 12)   [entry 2 spans 1 lines]",
]);
assert_eq!(session.entry(1).map(|e| e.name()), Some("<repl:1>"));
# Ok::<(), repl_lang::SessionError>(())
```

<br>

### Using parser errors to detect continuation

`Input::is_incomplete` turns any parser that reports a missing token *at the end of input* into a continuation detector. Errors in the middle of the input mean the entry is wrong, not unfinished, so it completes and the errors are reported.

```rust
use diag_lang::{Diagnostic, Label, Severity};
use repl_lang::{Feed, Session, Span, Status};

// Stand-in for a parser: "expected expression" at the end when the text ends
// with an operator, "unexpected `)`" where a stray parenthesis appears.
fn parse(text: &str, base: u32) -> Vec<Diagnostic> {
    let trimmed = text.trim_end();
    let error = |at: usize, msg: &str| {
        let at = base + at as u32;
        Diagnostic::new(Severity::Error, msg.to_owned(), Label::new(Span::new(at, at), "here"))
    };
    if let Some(at) = trimmed.find(')') {
        vec![error(at, "unexpected `)`")]
    } else if trimmed.ends_with(['+', '*', '=']) {
        vec![error(trimmed.len(), "expected expression")]
    } else {
        Vec::new()
    }
}

let mut session = Session::new();
let mut run = |line: &str| {
    session.feed(line, |input| {
        let errors = parse(input.text(), input.base().to_u32());
        if input.is_incomplete(&errors) { Status::Incomplete } else { Status::Complete(errors.len()) }
    })
};

assert_eq!(run("let x = 1 +")?, Feed::Incomplete);         // error at the end: keep reading
assert!(matches!(run("    2")?, Feed::Complete { value: 0, .. }));
assert!(matches!(run("1 + ) 2")?, Feed::Complete { value: 1, .. })); // error mid-line: report it
# Ok::<(), repl_lang::SessionError>(())
```

<br>

### Rendering diagnostics across entries

Every committed entry lives in `Session::sources`, a `SourceMap` that `diag_lang` renders against. Spans the pipeline records are global from the start, so a diagnostic can point into any entry, including earlier ones.

```rust
use diag_lang::{Diagnostic, Label, Renderer, Severity};
use repl_lang::{Feed, Session, Span, Status};

let mut session = Session::new();
let mut first_definition = None;

for line in ["let limit = 10", "let limit = 20"] {
    let feed = session.feed(line, |input| {
        let name = Span::new(input.base().to_u32() + 4, input.base().to_u32() + 9);
        match first_definition {
            None => {
                first_definition = Some(name);
                Status::Complete(None)
            }
            Some(first) => Status::Complete(Some(
                Diagnostic::new(Severity::Error, "`limit` is already defined", Label::new(name, "redefined"))
                    .with_secondary(Label::new(first, "first defined here")),
            )),
        }
    })?;
    if let Feed::Complete { value: Some(diagnostic), .. } = feed {
        let text = Renderer::new().render(&diagnostic, session.sources());
        assert!(text.contains("<repl:2>:1:5"));
        assert!(text.contains("<repl:1>:1:5"));
        assert!(text.contains("first defined here"));
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

<br>

### Line editing

The editor never touches a terminal. Map key events to `Edit` commands, redraw from `line()` and `column()`, and call `submit()` on Enter.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("let total = price");

assert!(editor.apply(Edit::KillWordLeft));   // Ctrl-W
editor.insert("cost * 2");
assert_eq!(editor.line(), "let total = cost * 2");

assert!(editor.apply(Edit::Home));           // Ctrl-A
assert!(editor.apply(Edit::WordRight));      // Alt-F: after "let"
assert_eq!(editor.cursor(), 3);

assert_eq!(editor.submit(), "let total = cost * 2");
assert!(editor.apply(Edit::HistoryPrev));    // Up: recall it
assert_eq!(editor.line(), "let total = cost * 2");

// Columns, not bytes, for placing the terminal cursor.
let mut wide = Editor::new();
wide.insert("名前 = 1");
assert_eq!(wide.cursor(), 10);
assert_eq!(wide.column(), 8);
```

<hr>
<br>

## Examples

Three runnable examples ship in [`examples/`](./examples). Two share a small calculator language in [`examples/common`](./examples/common/mod.rs): a lexer built on `Input::cursor`, a [`parser-lang`](https://crates.io/crates/parser-lang) Pratt grammar, `Input::is_incomplete` for continuation lines, and bindings that remember where they were defined. That module is the template for wiring repl-lang into a real language.

- **Calculator REPL** — an interactive loop over standard input with `let` bindings, multi-line entries, rendered diagnostics, and a `:history` command. It also runs non-interactively.
  ```bash
  cargo run --example calc
  echo "let x = (1 +
  2) * 7" | cargo run --example calc
  ```
- **Notebook** — a scripted session printed as `In [n]` / `Out[n]` cells: multi-line entries, blank lines that never consume a number, an interrupt, a redefinition error that underlines the original definition in an earlier cell, and the entry-size limit.
  ```bash
  cargo run --example notebook
  ```
- **Line editing** — replays a keystroke script through the editor and draws each state with a caret under the cursor column: word motion, kill and yank, a wide-character line, a combining accent kept with its letter, and history browsing that restores the half-typed line.
  ```bash
  cargo run --example line_editing
  ```

<hr>
<br>

## Performance

Feeding a line appends to one reused buffer and calls the pipeline; committing an entry makes one exact-size copy of its text and its name into the source map. The editor keeps its line, kill buffer, and every history slot between uses — once the history is full, submitting a line recycles the oldest slot instead of allocating. Display widths and word classes take an ASCII fast path before consulting the Unicode tables.

Measured with the benchmarks in [`benches/`](./benches) (x86_64, Rust stable, release profile):

| Benchmark | What it measures | Windows | Linux (WSL2) |
|---|---|---:|---:|
| `session/entries/100` | 100 single-line entries fed, checked, and committed. | ~6.5 µs | ~3.8 µs |
| `session/continuation/lines=8` | One entry over 8 lines, re-checked after each. | ~0.47 µs | ~0.27 µs |
| `session/feed_incomplete/errors=4` | Feed a line, run `is_incomplete` over 4 errors, cancel. | ~81 ns | ~48 ns |
| `editor/type/80` | Typing an 80-column line key by key. | ~0.36 µs | ~0.23 µs |
| `editor/word_motion/line=800` | Word-left to the start and word-right to the end of a long line. | ~7.4 µs | ~4.9 µs |
| `editor/submit/history_full` | Submitting into a full 100-entry history ring. | ~32 ns | ~23 ns |
| `editor/column/line=900` | Cursor column at the end of a long mixed-width line. | ~1.7 µs | ~1.4 µs |

Per entry that is roughly 40–65 ns to feed, check, and commit a line, and 3–5 ns per keystroke.

Run them yourself:

```bash
cargo bench --bench bench
```

Criterion writes per-benchmark reports to `target/criterion/`. Numbers vary by CPU; use the trend across runs, not a single absolute.

<hr>
<br>

## Design notes

- **The parser is the oracle.** Completeness is a property of the language, so the session asks the language. There is no built-in bracket counter to disagree with your grammar about strings, comments, or significant indentation; a pipeline that already parses gets continuation lines from `is_incomplete` in one call.
- **Check and work are the same call.** `Status::Complete` carries the pipeline's result, so a finished entry is parsed once, not once to test and again to run.
- **Spans are final when they are made.** The pipeline learns its entry's global base before the entry is committed, and the session guarantees the entry lands exactly there. Nothing is rebased afterwards.
- **Errors never echo input.** `SessionError` reports sizes, not text, so a refused line cannot leak into a log through the error value.
- **The terminal stays outside.** Raw mode, key decoding, and drawing differ on every platform; editing logic does not. Keeping them apart makes the editor identical everywhere and testable without a terminal.
- **Display units, not code points.** The cursor moves over a visible character together with any combining marks attached to it, so it never separates a letter from its accent, and `column()` counts wide characters as two terminal columns.

<hr>
<br>

## Testing

The suite runs on Windows, Linux (WSL2 Ubuntu), and macOS through the CI matrix, on stable and the 1.85 MSRV:

```bash
cargo test                       # unit + integration + property + doctests
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench bench
```

The property tests in [`tests/proptests.rs`](./tests/proptests.rs) hold both halves to reference models. Arbitrary sequences of edits, pastes, submits, and history operations — over text mixing ASCII, accents, combining marks, joiners, wide characters, emoji, and control characters — must leave the editor in exactly the state of a model that works on a `Vec<char>`. Arbitrary lines with arbitrary terminators and size limits must leave the session agreeing with a plain string accumulator on every pending buffer, entry number, base offset, span, and refusal. Every `rust` example in this README and in [`docs/API.md`](./docs/API.md) is compiled and run as a doctest, so the published examples cannot drift from the API.

<hr>
<br>

## Cross-platform support

- Linux (x86_64, aarch64)
- macOS (x86_64, Apple Silicon)
- Windows (x86_64)

The crate uses no operating-system facilities and no platform-specific code; behaviour is identical on every target. Line terminators are normalised on input, so `"\r\n"` from a Windows console and `"\n"` from a Unix pipe produce the same entry.

<hr>
<br>

## Contributing

See [`REPS.md`](./REPS.md) for the engineering standards every change is held to, and [`dev/ROADMAP.md`](./dev/ROADMAP.md) for the plan to 1.0. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>
