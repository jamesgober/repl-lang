# repl-lang &mdash; API Reference

> Complete reference for every public item in `repl-lang`, with examples.
> **Status: stable (1.0).** The surface below is the `1.0` contract; it follows
> [Semantic Versioning](#stability) and will not change in a breaking way before
> `2.0`. See [`../dev/ROADMAP.md`](../dev/ROADMAP.md).

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

## Table of contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Concepts](#concepts)
  - [The feed loop](#the-feed-loop)
  - [Deciding completeness](#deciding-completeness)
  - [Positions and the source map](#positions-and-the-source-map)
  - [Display units and words](#display-units-and-words)
  - [History and the draft](#history-and-the-draft)
- [`Session`](#session)
  - [`Session::DEFAULT_LIMIT`](#sessiondefault_limit)
  - [`Session::new`](#sessionnew)
  - [`Session::with_limit`](#sessionwith_limit)
  - [`Session::feed`](#sessionfeed)
  - [`Session::cancel`](#sessioncancel)
  - [`Session::is_pending`](#sessionis_pending)
  - [`Session::pending`](#sessionpending)
  - [`Session::len`](#sessionlen)
  - [`Session::is_empty`](#sessionis_empty)
  - [`Session::entry`](#sessionentry)
  - [`Session::sources`](#sessionsources)
  - [`Session::limit`](#sessionlimit)
- [`Input`](#input)
  - [`Input::text`](#inputtext)
  - [`Input::base`](#inputbase)
  - [`Input::span`](#inputspan)
  - [`Input::number`](#inputnumber)
  - [`Input::cursor`](#inputcursor)
  - [`Input::is_incomplete`](#inputis_incomplete)
- [`Status`](#status)
- [`Feed`](#feed)
- [`Entry`](#entry)
- [`SessionError`](#sessionerror)
- [`Editor`](#editor)
  - [`Editor::DEFAULT_HISTORY`](#editordefault_history)
  - [`Editor::new`](#editornew)
  - [`Editor::with_history`](#editorwith_history)
  - [`Editor::line`](#editorline)
  - [`Editor::cursor`](#editorcursor)
  - [`Editor::column`](#editorcolumn)
  - [`Editor::apply`](#editorapply)
  - [`Editor::insert`](#editorinsert)
  - [`Editor::clear`](#editorclear)
  - [`Editor::submit`](#editorsubmit)
  - [`Editor::add_history`](#editoradd_history)
  - [`Editor::history`](#editorhistory)
- [`Edit`](#edit)
- [Re-exports](#re-exports)
- [Feature flags](#feature-flags)
- [Guide: wiring a parser](#guide-wiring-a-parser)
- [Guide: driving the editor from a terminal](#guide-driving-the-editor-from-a-terminal)
- [Stability](#stability)

---

## Overview

repl-lang is the state of a read-eval-print loop with the language and the
terminal left out. It has two halves that work together or apart:

| Item | Role |
|---|---|
| [`Session`](#session) | Accumulates lines into entries, runs the pipeline, commits finished entries. |
| [`Input`](#input) | What the pipeline sees: the entry so far and its global position. |
| [`Status`](#status) | The pipeline's answer: complete with a value, or incomplete. |
| [`Feed`](#feed) | The session's answer to one line. |
| [`Entry`](#entry) | A handle to a committed entry. |
| [`SessionError`](#sessionerror) | Why a line was refused. |
| [`Editor`](#editor) | The line being edited, its cursor, a kill buffer, and a history. |
| [`Edit`](#edit) | One editing command. |

The crate is `#![forbid(unsafe_code)]`, `no_std`-compatible (needs only
`alloc`), and depends on [`lexer-lang`](https://crates.io/crates/lexer-lang)
(`Cursor`), [`diag-lang`](https://crates.io/crates/diag-lang) (`Diagnostic`),
[`source-lang`](https://crates.io/crates/source-lang) (`SourceMap`), and
[`unicode-lang`](https://crates.io/crates/unicode-lang) (display widths and
identifier classes).

---

## Installation

```toml
[dependencies]
repl-lang = "1"
diag-lang = "1"   # to build and render diagnostics
```

Or from the terminal:

```bash
cargo add repl-lang diag-lang
```

MSRV: Rust 1.85 (Rust 2024 edition).

---

## Quick start

A pipeline that waits for balanced brackets, fed three lines:

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let pipeline = |input: repl_lang::Input<'_>| {
    let open = input.text().matches('[').count();
    let close = input.text().matches(']').count();
    if open > close { Status::Incomplete } else { Status::Complete(input.text().to_owned()) }
};

assert_eq!(session.feed("[1, 2,", pipeline)?, Feed::Incomplete);
assert!(session.is_pending());
let Feed::Complete { entry, value } = session.feed(" 3]", pipeline)? else {
    return Err("expected a complete entry".into());
};
assert_eq!(value, "[1, 2,\n 3]\n");
assert_eq!(entry.number(), 1);
assert_eq!(session.feed("", pipeline)?, Feed::Empty);
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## Concepts

### The feed loop

A REPL host reads a line, hands it to [`Session::feed`](#sessionfeed) with a
*pipeline* closure, and acts on the [`Feed`](#feed) it gets back:

1. A blank line with nothing pending returns `Feed::Empty` without calling the
   pipeline — the host just prompts again.
2. Otherwise the line joins the pending entry, and the pipeline runs on the
   **whole** entry so far, as an [`Input`](#input).
3. `Status::Incomplete` keeps the entry pending (`Feed::Incomplete`); the host
   shows a continuation prompt.
4. `Status::Complete(value)` commits the entry and returns
   `Feed::Complete { entry, value }`; the host prints the value or the errors.

The pipeline is an `FnOnce` closure, so it can borrow the interpreter state
mutably for the duration of one call. The session never stores it.

### Deciding completeness

Only the language knows whether `let f = fn(x) {` is finished, so the session
asks the pipeline. A pipeline that already parses has the answer in its error
list: when the parser runs out of input mid-construct it reports the missing
piece *at the end of the input*. [`Input::is_incomplete`](#inputis_incomplete)
checks exactly that — at least one error, and every error at or after the last
non-whitespace byte. Errors anywhere else mean the input is wrong, and typing
more will not help, so the entry completes and the errors are shown.

Return `Status::Complete` for wrong input as well as right input. Only input
that stopped too soon is `Incomplete`.

### Positions and the source map

Every committed entry becomes one source in the session's
[`SourceMap`](#sessionsources), named `<repl:N>` and laid out after the
entries before it in a single 32-bit position space. Entry 1 occupies
`0..len₁`, entry 2 `len₁..len₁+len₂`, and so on.

The pipeline is told its entry's position before the entry is committed:
[`Input::base`](#inputbase) is where its first byte will land, and the session
guarantees it lands there. A pipeline that lexes with
[`Input::cursor`](#inputcursor) therefore produces spans that are valid in the
session's map from the moment they are made. Diagnostics can point at the
current entry, at earlier entries, or at both at once, and
`diag_lang::Renderer::render(&diagnostic, session.sources())` resolves each
span to the right `<repl:N>`, line, and column.

### Display units and words

The editor moves and deletes in **display units**: a character with a visible
width followed by every zero-width character attached to it (combining marks,
variation selectors, joiners). `e` followed by `U+0301 COMBINING ACUTE ACCENT`
is one unit, so the cursor never sits between a letter and its accent and one
backspace removes what the user sees as one character.

A **word** is a run of units whose base character is an identifier character
(Unicode `XID_Continue`: letters, digits, `_`, combining marks). Everything else
— spaces, operators, punctuation — separates words.

[`Editor::column`](#editorcolumn) measures the text before the cursor in
terminal columns: wide characters such as CJK ideographs and most emoji count
two, combining marks count none.

### History and the draft

[`Editor::submit`](#editorsubmit) records each finished line in a bounded
history, skipping blank lines and immediate repeats. Browsing with
[`Edit::HistoryPrev`](#edit) first parks the line being typed (the *draft*),
then replaces the line with older entries; [`Edit::HistoryNext`](#edit) walks
forward and, past the newest entry, brings the draft back. Editing a recalled
line edits a copy — the history itself never changes. Submitting while
browsing submits the recalled line as edited and drops the draft.

---

## `Session`

```rust,ignore
pub struct Session { /* private fields */ }
```

The state of one REPL session: the entry being typed and every entry accepted
so far. It owns the decision loop, not the language: each
[`feed`](#sessionfeed) appends a line and runs the caller's pipeline on the
whole entry. Completed entries are committed to a [`SourceMap`](#sessionsources)
as `<repl:N>`.

The session does no I/O. Pair it with an [`Editor`](#editor), with `stdin`
read line by line, or with a script.

**Cost.** While an entry is incomplete the pipeline re-reads it in full for
every new line, so an entry of *k* lines is processed *k* times. Interactive
entries are short, and this keeps the pipeline stateless. Feed a large paste as
one chunk — `feed` accepts text containing newlines — and cap entry size with
[`with_limit`](#sessionwith_limit). The pending buffer is reused across entries;
committing an entry makes one exact-size copy of its text and its name.

`Session` implements `Clone`, `Debug`, and `Default` (same as
[`Session::new`](#sessionnew)), and is `Send + Sync`.

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let mut lines_per_entry = Vec::new();

for line in ["(define (square x)", "  (* x x))", "(square 4)"] {
    let feed = session.feed(line, |input| {
        let depth: i32 = input.text().chars().map(|c| match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        }).sum();
        if depth > 0 { Status::Incomplete } else { Status::Complete(input.text().lines().count()) }
    })?;
    if let Feed::Complete { value, .. } = feed {
        lines_per_entry.push(value);
    }
}
assert_eq!(lines_per_entry, [2, 1]);
assert_eq!(session.len(), 2);
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::DEFAULT_LIMIT`

```rust,ignore
pub const DEFAULT_LIMIT: usize = 1 << 20;
```

The entry size limit of [`Session::new`](#sessionnew): 1 MiB, counting the
`'\n'` that ends each line.

```rust
use repl_lang::Session;

assert_eq!(Session::DEFAULT_LIMIT, 1_048_576);
assert_eq!(Session::new().limit(), Session::DEFAULT_LIMIT);
```

### `Session::new`

```rust,ignore
pub fn new() -> Session
```

Creates a session with no entries and the
[`DEFAULT_LIMIT`](#sessiondefault_limit). Nothing is allocated until the first
line is fed.

```rust
use repl_lang::Session;

let session = Session::new();
assert!(session.is_empty());
assert!(!session.is_pending());
assert!(session.sources().is_empty());
```

### `Session::with_limit`

```rust,ignore
pub fn with_limit(limit: usize) -> Session
```

Creates a session whose entries may grow to at most `limit` bytes.

| Parameter | Meaning |
|---|---|
| `limit` | Maximum byte length of one entry, including the `'\n'` that ends each line. `0` refuses every non-blank line; `usize::MAX` disables the limit (the 4 GiB source-map space still bounds the session). |

A line that would take the pending entry past the limit is refused with
[`SessionError::TooLong`](#sessionerror) and the pending entry is discarded.
The limit bounds both the memory one entry can hold and the work the pipeline
repeats on each continuation line, so lower it when input is untrusted.

```rust
use repl_lang::{Feed, Session, SessionError, Status};

let mut session = Session::with_limit(16);
let wait = |_: repl_lang::Input<'_>| Status::<()>::Incomplete;

assert_eq!(session.feed("0123456789", wait)?, Feed::Incomplete); // 11 bytes
assert_eq!(
    session.feed("abcdef", wait),                                // would be 18
    Err(SessionError::TooLong { len: 18, limit: 16 }),
);
assert!(!session.is_pending()); // the oversized entry was dropped
# Ok::<(), SessionError>(())
```

An entry that lands exactly on the limit is accepted:

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::with_limit(4);
assert!(matches!(session.feed("abc", |_| Status::Complete(())), Ok(Feed::Complete { .. })));
```

### `Session::feed`

```rust,ignore
pub fn feed<T, F>(&mut self, line: &str, pipeline: F) -> Result<Feed<T>, SessionError>
where
    F: FnOnce(Input<'_>) -> Status<T>,
```

Appends a line to the pending entry and runs `pipeline` on the whole entry.

| Parameter | Meaning |
|---|---|
| `line` | One line of input, with or without its terminator. A single trailing `"\n"` or `"\r\n"` is removed and a `'\n'` appended, so `read_line` output and `str::lines` items behave the same. It may contain several lines, as from a paste. |
| `pipeline` | The language: called once with an [`Input`](#input) over the entry so far, returning a [`Status`](#status). Not called for a blank line with nothing pending, nor when the line is refused. |

**Returns** [`Feed::Empty`](#feed) for a blank line with nothing pending,
[`Feed::Incomplete`](#feed) when the pipeline asks for more, or
[`Feed::Complete`](#feed) with the committed [`Entry`](#entry) and the
pipeline's value. Blank lines inside a pending entry are passed to the
pipeline, so a language can use one to end a block.

**Errors.** [`SessionError::TooLong`](#sessionerror) if the entry would exceed
the limit; [`SessionError::SpaceExhausted`](#sessionerror) if the source map
cannot hold it. Either way the pending entry is discarded and the pipeline is
not called.

Lines straight from `read_line`, terminators included:

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let mut last = None;
for raw in ["let x =\r\n", "  42\n"] {
    last = Some(session.feed(raw, |input| {
        if input.text().trim_end().ends_with('=') {
            Status::Incomplete
        } else {
            Status::Complete(input.text().to_owned())
        }
    })?);
}
let Some(Feed::Complete { value, .. }) = last else { return Err("incomplete".into()) };
assert_eq!(value, "let x =\n  42\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

A pipeline that borrows interpreter state mutably:

```rust
use repl_lang::{Feed, Session, Status};

let mut total = 0_i64;
let mut session = Session::new();
for line in ["5", "10", "oops", "-3"] {
    let feed = session.feed(line, |input| match input.text().trim().parse::<i64>() {
        Ok(n) => {
            total += n;
            Status::Complete(Ok(total))
        }
        Err(e) => Status::Complete(Err(e.to_string())),
    })?;
    if let Feed::Complete { value: Err(message), .. } = feed {
        assert_eq!(message, "invalid digit found in string");
    }
}
assert_eq!(total, 12);
# Ok::<(), repl_lang::SessionError>(())
```

A pasted block fed as one chunk:

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let paste = "fn add(a, b) {\n    a + b\n}\n";
let feed = session.feed(paste, |input| Status::Complete(input.text().lines().count()))?;
assert!(matches!(feed, Feed::Complete { value: 3, .. }));
assert_eq!(session.entry(1).map(|e| e.text()), Some(paste));
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::cancel`

```rust,ignore
pub fn cancel(&mut self) -> bool
```

Discards the pending entry; returns `true` if there was one. This is the
session half of an interrupt (`Ctrl-C`). Committed entries are untouched and
the next entry number does not change.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
session.feed("if ready {", |_| Status::<()>::Incomplete)?;
assert!(session.cancel());
assert!(!session.is_pending());
assert!(!session.cancel()); // nothing left to discard

// The next entry is still number 1.
session.feed("ok", |input| {
    assert_eq!(input.number(), 1);
    Status::Complete(())
})?;
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::is_pending`

```rust,ignore
pub fn is_pending(&self) -> bool
```

`true` while an entry is incomplete — when the next line continues it. Use it
to choose between the primary and the continuation prompt.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
let prompt = |s: &Session| if s.is_pending() { "... " } else { ">>> " };

assert_eq!(prompt(&session), ">>> ");
session.feed("[1,", |_| Status::<()>::Incomplete)?;
assert_eq!(prompt(&session), "... ");
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::pending`

```rust,ignore
pub fn pending(&self) -> &str
```

The text of the pending entry: every line fed since the last completed entry,
each ending in `'\n'`. Empty when nothing is pending. Useful for redrawing a
multi-line entry, or for reporting what was lost when input ends mid-entry.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
session.feed("a", |_| Status::<()>::Incomplete)?;
session.feed("b\r\n", |_| Status::<()>::Incomplete)?;
assert_eq!(session.pending(), "a\nb\n");
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::len`

```rust,ignore
pub fn len(&self) -> usize
```

The number of committed entries. Blank lines, pending entries, and refused
lines do not count. `len() + 1` is the number the next entry will receive,
which suits numbered prompts such as `In [3]:`.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
session.feed("one", |_| Status::Complete(()))?;
session.feed("", |_| Status::Complete(()))?;        // blank: not an entry
session.feed("two (", |_| Status::<()>::Incomplete)?; // pending: not yet
assert_eq!(session.len(), 1);
assert_eq!(format!("In [{}]:", session.len() + 1), "In [2]:");
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::is_empty`

```rust,ignore
pub fn is_empty(&self) -> bool
```

`true` if no entry has been committed yet.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
assert!(session.is_empty());
session.feed("x", |_| Status::Complete(()))?;
assert!(!session.is_empty());
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::entry`

```rust,ignore
pub fn entry(&self, number: u32) -> Option<&SourceFile>
```

The committed entry with the given number, or `None` if there is none.

| Parameter | Meaning |
|---|---|
| `number` | The entry number, counting from `1`, as reported by [`Entry::number`](#entry) and [`Input::number`](#inputnumber). `0` and numbers past [`len`](#sessionlen) return `None`. |

The [`SourceFile`](#re-exports) gives the entry's name (`<repl:N>`), its text,
and its global span — what a `:history` or `:show` command needs. Lookup is
constant time.

```rust
use repl_lang::{Session, Span, Status};

let mut session = Session::new();
session.feed("x = 1", |_| Status::Complete(()))?;
session.feed("y = 2", |_| Status::Complete(()))?;

let second = session.entry(2).ok_or("missing")?;
assert_eq!(second.name(), "<repl:2>");
assert_eq!(second.text(), "y = 2\n");
assert_eq!(second.span(), Span::new(6, 12));
assert!(session.entry(0).is_none());
assert!(session.entry(3).is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Listing the whole session, as a `:history` command would:

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
for line in ["let a = 1", "let b = 2", "a + b"] {
    session.feed(line, |_| Status::Complete(()))?;
}
let listing: Vec<String> = (1..=session.len() as u32)
    .filter_map(|n| session.entry(n))
    .map(|e| format!("{} {}", e.name(), e.text().trim_end()))
    .collect();
assert_eq!(listing, ["<repl:1> let a = 1", "<repl:2> let b = 2", "<repl:3> a + b"]);
# Ok::<(), repl_lang::SessionError>(())
```

### `Session::sources`

```rust,ignore
pub fn sources(&self) -> &SourceMap
```

The source map holding every committed entry, one source per entry, in entry
order. Render diagnostics against it: any span the pipeline produced from an
[`Input`](#input) resolves to the right entry, line, and column.

```rust
use diag_lang::{Diagnostic, Label, Renderer, Severity};
use repl_lang::{Feed, Session, Span, Status};

let mut session = Session::new();
let feed = session.feed("let y = x + 1", |input| {
    // Pretend `x` is unbound and point at it with a global span.
    let at = input.base().to_u32() + 8;
    Status::Complete(Diagnostic::new(
        Severity::Error,
        "cannot find `x`",
        Label::new(Span::new(at, at + 1), "not found"),
    ))
})?;
let Feed::Complete { value: diagnostic, .. } = feed else { return Err("pending".into()) };

let text = Renderer::new().render(&diagnostic, session.sources());
assert!(text.contains("<repl:1>:1:9"));
assert!(text.contains("^ not found"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Resolving a global position back to its entry:

```rust
use repl_lang::{BytePos, Session, Status};

let mut session = Session::new();
session.feed("first", |_| Status::Complete(()))?;  // 0..6
session.feed("second", |_| Status::Complete(()))?; // 6..13

let (id, local) = session.sources().locate(BytePos::new(8)).ok_or("outside")?;
assert_eq!(session.sources().source(id).map(|s| s.name()), Some("<repl:2>"));
assert_eq!(local, BytePos::new(2));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `Session::limit`

```rust,ignore
pub fn limit(&self) -> usize
```

The maximum byte length of one entry, as given to
[`with_limit`](#sessionwith_limit).

```rust
use repl_lang::Session;

assert_eq!(Session::with_limit(4096).limit(), 4096);
```

---

## `Input`

```rust,ignore
pub struct Input<'a> { /* private fields */ }
```

The pending text of one entry, as the pipeline sees it: every line of the entry
so far, joined with `'\n'` and ending with one, plus its position in the
session's source map. `Input` is a borrowed `Copy` view that lives only for the
duration of the pipeline call. It implements `Clone`, `Copy`, `Debug`,
`PartialEq`, and `Eq`.

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let feed = session.feed("1 + 2", |input| {
    assert_eq!(input.text(), "1 + 2\n");
    assert_eq!(input.number(), 1);
    Status::Complete(input.span().len())
})?;
assert!(matches!(feed, Feed::Complete { value: 6, .. }));
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::text`

```rust,ignore
pub const fn text(&self) -> &'a str
```

Every line of the entry so far, joined with `'\n'` and ending with one. The
first call sees one line, the next sees two, and so on, until the pipeline
answers `Complete`.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
let mut seen = Vec::new();
for line in ["(1 +", " 2)"] {
    session.feed(line, |input| {
        seen.push(input.text().to_owned());
        if input.text().matches('(').count() > input.text().matches(')').count() {
            Status::Incomplete
        } else {
            Status::Complete(())
        }
    })?;
}
assert_eq!(seen, ["(1 +\n", "(1 +\n 2)\n"]);
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::base`

```rust,ignore
pub const fn base(&self) -> BytePos
```

The global position the entry's first byte occupies in the session's source
map. Entries are laid out end to end, so the base of entry *n* is the total
length of entries 1 through *n − 1*. Add it to a local byte offset to get a
global position.

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
session.feed("abc", |_| Status::Complete(()))?; // occupies 0..4 ("abc\n")
session.feed("xyz", |input| {
    assert_eq!(input.base().to_u32(), 4);
    let local = input.text().find('y').unwrap_or(0) as u32;
    assert_eq!(input.base().to_u32() + local, 5); // global position of `y`
    Status::Complete(())
})?;
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::span`

```rust,ignore
pub const fn span(&self) -> Span
```

The global span the whole entry occupies: from [`base`](#inputbase) to the end
of [`text`](#inputtext). Once committed, the [`Entry`](#entry) reports the same
span.

```rust
use repl_lang::{Feed, Session, Span, Status};

let mut session = Session::new();
let mut seen = Span::empty(0);
let feed = session.feed("let x = 1", |input| {
    seen = input.span();
    Status::Complete(())
})?;
assert_eq!(seen, Span::new(0, 10));
assert!(matches!(feed, Feed::Complete { entry, .. } if entry.span() == seen));
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::number`

```rust,ignore
pub const fn number(&self) -> u32
```

The number the entry will be committed as, counting from `1`. It does not
advance while an entry is incomplete, and blank lines never consume one —
suitable for prompts (`In [3]:`) and for names an evaluator binds per entry
(`_3`).

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
let mut numbers = Vec::new();
for (line, done) in [("a (", false), ("b)", true), ("", true), ("c", true)] {
    session.feed(line, |input| {
        numbers.push(input.number());
        if done { Status::Complete(()) } else { Status::Incomplete }
    })?;
}
assert_eq!(numbers, [1, 1, 2]); // the blank line never reached the pipeline
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::cursor`

```rust,ignore
pub fn cursor(&self) -> lexer_lang::Cursor<'a>
```

A [`lexer_lang::Cursor`](https://docs.rs/lexer-lang) over [`text`](#inputtext),
positioned at [`base`](#inputbase). Every token span it produces is a global
span in the session's source map, so tokens, the syntax built from them, and
any diagnostic that points at them render correctly against
[`Session::sources`](#sessionsources).

```rust
use repl_lang::{Session, Status};

let mut session = Session::new();
session.feed("first", |_| Status::Complete(()))?; // 0..6
session.feed("ab cd", |input| {
    let mut cursor = input.cursor();
    cursor.eat_while(|c| c.is_alphabetic());
    assert_eq!(cursor.lexeme(), "ab");
    assert_eq!(cursor.token_span().start().to_u32(), 6); // global, not 0
    Status::Complete(())
})?;
# Ok::<(), repl_lang::SessionError>(())
```

A whole lexer on top of it, splitting words and skipping spaces:

```rust
use repl_lang::{Session, Span, Status};

let mut session = Session::new();
session.feed("seed", |_| Status::Complete(()))?; // 0..5
session.feed("let x", |input| {
    let mut cursor = input.cursor();
    let mut words: Vec<(String, Span)> = Vec::new();
    while let Some(c) = cursor.bump() {
        if c.is_whitespace() {
            cursor.reset_token();
            continue;
        }
        cursor.eat_while(|c| !c.is_whitespace());
        words.push((cursor.lexeme().to_owned(), cursor.token_span()));
        cursor.reset_token();
    }
    assert_eq!(words, [("let".to_owned(), Span::new(5, 8)), ("x".to_owned(), Span::new(9, 10))]);
    Status::Complete(())
})?;
# Ok::<(), repl_lang::SessionError>(())
```

### `Input::is_incomplete`

```rust,ignore
pub fn is_incomplete(&self, errors: &[Diagnostic]) -> bool
```

`true` if `errors` say the input ended too soon rather than that it is wrong —
the signal to ask for another line.

| Parameter | Meaning |
|---|---|
| `errors` | The diagnostics the pipeline's parser produced for this input, with global spans. Only `Severity::Error` entries are considered; warnings, notes, and help are ignored. |

**Returns** `true` when there is at least one error and every error's primary
span starts at or after the end of the meaningful text (the last
non-whitespace byte). A parser that reaches the end of `1 + (2 *` reports
"expected expression" at the end; a parser that trips over `1 + )` reports the
error at the `)`, and no further line can fix that.

The rule matches how [`parser_lang`](https://docs.rs/parser-lang) reports a
missing token at the end of input — an empty span just past the last token —
and works with any parser that does the same. Errors a lexer reports where a
construct *starts* (an unterminated string at its opening quote, say) are not
recognised; return `Status::Incomplete` for those directly.

```rust
use diag_lang::{Diagnostic, Label, Severity};
use repl_lang::{Session, Span, Status};

let at = |pos: u32, severity: Severity| {
    Diagnostic::new(severity, "expected expression", Label::new(Span::empty(pos), "here"))
};

let mut session = Session::new();
session.feed("1 + (2 *  ", |input| {
    // The meaningful text ends at byte 8; trailing spaces and the newline do not count.
    assert!(input.is_incomplete(&[at(8, Severity::Error)]));
    assert!(input.is_incomplete(&[at(11, Severity::Error)]));
    // An error in the middle: wrong, not unfinished.
    assert!(!input.is_incomplete(&[at(2, Severity::Error), at(8, Severity::Error)]));
    // No errors at all, or only warnings: complete.
    assert!(!input.is_incomplete(&[]));
    assert!(!input.is_incomplete(&[at(8, Severity::Warning)]));
    Status::Complete(())
})?;
# Ok::<(), repl_lang::SessionError>(())
```

Combined with a lexer that reports unterminated strings itself:

```rust
use diag_lang::Diagnostic;
use repl_lang::{Feed, Session, Status};

fn pipeline(input: repl_lang::Input<'_>, errors: &[Diagnostic]) -> Status<()> {
    let open_string = input.text().matches('"').count() % 2 == 1;
    if open_string || input.is_incomplete(errors) { Status::Incomplete } else { Status::Complete(()) }
}

let mut session = Session::new();
assert_eq!(session.feed("print \"hello", |input| pipeline(input, &[]))?, Feed::Incomplete);
assert!(matches!(session.feed("world\"", |input| pipeline(input, &[]))?, Feed::Complete { .. }));
# Ok::<(), repl_lang::SessionError>(())
```

---

## `Status`

```rust,ignore
pub enum Status<T> {
    Complete(T),
    Incomplete,
}
```

The pipeline's verdict on the pending input.

| Variant | Meaning |
|---|---|
| `Complete(T)` | The input is a whole entry. The session commits it and returns `T` in [`Feed::Complete`](#feed). `T` is whatever the pipeline produced: a value, a tree, a list of diagnostics, a `Result`. |
| `Incomplete` | The input stops partway through a construct. The session keeps it and calls the pipeline again with the next line appended. |

Because the completeness check and the real work are the same call, a finished
entry is parsed once. Return `Complete` for wrong input too; only input that
ended too soon is `Incomplete`. `Status` implements `Clone`, `Copy` (when `T`
does), `Debug`, `PartialEq`, `Eq`, and `Hash`.

```rust
use repl_lang::{Feed, Session, Status};

// A trailing backslash continues the line, shell-style.
let pipeline = |input: repl_lang::Input<'_>| {
    if input.text().trim_end().ends_with('\\') {
        Status::Incomplete
    } else {
        Status::Complete(input.text().lines().count())
    }
};

let mut session = Session::new();
assert_eq!(session.feed("echo one \\", pipeline)?, Feed::Incomplete);
assert!(matches!(session.feed("two", pipeline)?, Feed::Complete { value: 2, .. }));
# Ok::<(), repl_lang::SessionError>(())
```

Carrying a `Result`, so errors flow through the same channel as values:

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let feed = session.feed("10 / 0", |input| {
    let parts: Vec<i64> = input.text().split('/').filter_map(|p| p.trim().parse().ok()).collect();
    Status::Complete(match parts[..] {
        [_, 0] => Err("division by zero"),
        [a, b] => Ok(a / b),
        _ => Err("expected `a / b`"),
    })
})?;
assert!(matches!(feed, Feed::Complete { value: Err("division by zero"), .. }));
# Ok::<(), repl_lang::SessionError>(())
```

---

## `Feed`

```rust,ignore
pub enum Feed<T> {
    Complete { entry: Entry, value: T },
    Incomplete,
    Empty,
}
```

The outcome of feeding one line.

| Variant | Meaning | Typical host action |
|---|---|---|
| `Complete { entry, value }` | The pipeline accepted the entry; it is committed. `entry` locates it, `value` is what the pipeline returned. | Print the value or render the errors. |
| `Incomplete` | The pipeline needs more input; the line is kept. | Show the continuation prompt. |
| `Empty` | The line was blank and nothing was pending. The pipeline was not called and no number was used. | Prompt again. |

`Feed` implements `Clone`, `Copy` (when `T` does), `Debug`, `PartialEq`, `Eq`,
and `Hash`.

```rust
use repl_lang::{Feed, Session, Status};

let mut session = Session::new();
let pipeline = |input: repl_lang::Input<'_>| {
    let open = input.text().matches('{').count();
    let close = input.text().matches('}').count();
    if open > close { Status::Incomplete } else { Status::Complete(input.text().len()) }
};

let mut transcript = Vec::new();
for line in ["", "f {", "  x", "}"] {
    let prompt = if session.is_pending() { "... " } else { ">>> " };
    match session.feed(line, pipeline)? {
        Feed::Complete { entry, value } => {
            transcript.push(format!("{prompt}{line}  -> entry {} ({value} bytes)", entry.number()));
        }
        Feed::Incomplete => transcript.push(format!("{prompt}{line}")),
        Feed::Empty => transcript.push(prompt.trim_end().to_owned()),
    }
}
assert_eq!(transcript, [">>>", ">>> f {", "...   x", "... }  -> entry 1 (10 bytes)"]);
# Ok::<(), repl_lang::SessionError>(())
```

---

## `Entry`

```rust,ignore
pub struct Entry { /* private fields */ }

impl Entry {
    pub const fn number(&self) -> u32;
    pub const fn id(&self) -> SourceId;
    pub const fn span(&self) -> Span;
}
```

A committed entry: a small `Copy` handle returned in
[`Feed::Complete`](#feed). The text itself is stored once, in the session.

| Method | Returns |
|---|---|
| `number()` | The entry number, counting from `1`; the key for [`Session::entry`](#sessionentry). Equal to the [`Input::number`](#inputnumber) the pipeline saw. |
| `id()` | The entry's [`SourceId`](#re-exports) in [`Session::sources`](#sessionsources). |
| `span()` | The global span of the entry's text; equal to the [`Input::span`](#inputspan) the pipeline saw. |

`Entry` implements `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, and `Hash`.

```rust
use repl_lang::{Feed, Session, Span, Status};

let mut session = Session::new();
session.feed("first", |_| Status::Complete(()))?;
let Feed::Complete { entry, .. } = session.feed("second", |_| Status::Complete(()))? else {
    return Err("the pipeline always completes".into());
};

assert_eq!(entry.number(), 2);
assert_eq!(entry.span(), Span::new(6, 13));
let source = session.sources().source(entry.id()).ok_or("missing")?;
assert_eq!(source.name(), "<repl:2>");
assert_eq!(source.text(), "second\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## `SessionError`

```rust,ignore
#[non_exhaustive]
pub enum SessionError {
    TooLong { len: usize, limit: usize },
    SpaceExhausted { needed: u64, available: u64 },
}
```

Why a session refused a line. Both variants mean the pending entry was too
large to keep, so the session discards it before returning: the next line
starts a fresh entry and the session stays usable. Committed entries are
unaffected, and the pipeline was not called. The error reports sizes only,
never the refused text.

| Variant | Meaning | What to do |
|---|---|---|
| `TooLong { len, limit }` | The entry would reach `len` bytes, past the session's `limit`. | Report it; raise the limit with [`with_limit`](#sessionwith_limit) if legitimate entries hit it. |
| `SpaceExhausted { needed, available }` | The session's source map has only `available` bytes of its 4 GiB position space left, and the entry needs `needed`. | The session is effectively full; start a new one. |

`SessionError` implements `Clone`, `Debug`, `PartialEq`, `Eq`, `Display`, and
`core::error::Error`. It is `#[non_exhaustive]`: match it with a wildcard arm.

```rust
use repl_lang::{Session, SessionError, Status};

let mut session = Session::with_limit(8);
let err = session.feed("far too long for eight bytes", |_| Status::Complete(())).unwrap_err();
assert_eq!(err, SessionError::TooLong { len: 29, limit: 8 });
assert_eq!(err.to_string(), "entry too long: 29 bytes exceeds the session limit of 8 bytes");
assert!(!session.is_pending());
```

Handling it in a host loop:

```rust
use repl_lang::{Feed, Session, SessionError, Status};

let mut session = Session::with_limit(32);
let mut log = Vec::new();
for line in ["short", "this line is well over thirty-two bytes long", "short again"] {
    match session.feed(line, |_| Status::Complete(())) {
        Ok(Feed::Complete { entry, .. }) => log.push(format!("ok {}", entry.number())),
        Ok(_) => {}
        Err(error @ SessionError::TooLong { .. }) => log.push(format!("refused: {error}")),
        Err(error) => return Err(error.into()),
    }
}
assert_eq!(log[0], "ok 1");
assert!(log[1].starts_with("refused: entry too long"));
assert_eq!(log[2], "ok 2");
# Ok::<(), Box<dyn std::error::Error>>(())
```

---

## `Editor`

```rust,ignore
pub struct Editor { /* private fields */ }
```

The editing state of one input line: its text, the cursor, a kill buffer, and
a bounded history of submitted lines. The host translates key events into
[`Edit`](#edit) commands, calls [`apply`](#editorapply), and redraws from
[`line`](#editorline) and [`column`](#editorcolumn) when `apply` reports a
change; on Enter it calls [`submit`](#editorsubmit). The editor holds no file
descriptors and draws nothing, so it behaves identically on every platform.

**Invariants.** The cursor is a byte offset on a `char` boundary, never inside
a [display unit](#display-units-and-words). The line never contains control
characters: [`Edit::Insert`](#edit), [`insert`](#editorinsert), and
[`add_history`](#editoradd_history) all refuse them.

**Memory.** The line, the kill buffer, and every history slot are reused. Once
the history is full, a submit recycles the oldest slot's buffer, so a long
session edits and submits without allocating except to grow a buffer past its
previous high-water mark.

`Editor` implements `Clone`, `Debug`, and `Default` (same as
[`Editor::new`](#editornew)), and is `Send + Sync`.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("let x = 1");
assert_eq!(editor.submit(), "let x = 1");
assert_eq!(editor.line(), "");

assert!(editor.apply(Edit::HistoryPrev));
assert_eq!(editor.line(), "let x = 1");
```

### `Editor::DEFAULT_HISTORY`

```rust,ignore
pub const DEFAULT_HISTORY: usize = 1000;
```

The number of history entries [`Editor::new`](#editornew) keeps.

```rust
use repl_lang::Editor;

let mut editor = Editor::new();
for i in 0..1200 {
    editor.add_history(&format!("line {i}"));
}
assert_eq!(editor.history().len(), Editor::DEFAULT_HISTORY);
assert_eq!(editor.history().next(), Some("line 200"));
```

### `Editor::new`

```rust,ignore
pub fn new() -> Editor
```

Creates an empty editor keeping the last
[`DEFAULT_HISTORY`](#editordefault_history) submitted lines. Nothing is
allocated until text is inserted.

```rust
use repl_lang::Editor;

let editor = Editor::new();
assert_eq!(editor.line(), "");
assert_eq!(editor.cursor(), 0);
assert_eq!(editor.history().len(), 0);
```

### `Editor::with_history`

```rust,ignore
pub fn with_history(capacity: usize) -> Editor
```

Creates an empty editor that keeps at most `capacity` history entries.

| Parameter | Meaning |
|---|---|
| `capacity` | Maximum history entries. When full, recording a line drops the oldest. `0` disables history: nothing is recorded and the history commands do nothing. No memory is reserved up front. |

```rust
use repl_lang::Editor;

let mut editor = Editor::with_history(2);
for line in ["a", "b", "c"] {
    editor.insert(line);
    editor.submit();
}
assert!(editor.history().eq(["b", "c"]));
```

With history disabled:

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::with_history(0);
editor.insert("secret");
assert_eq!(editor.submit(), "secret");
assert_eq!(editor.history().len(), 0);
assert!(!editor.apply(Edit::HistoryPrev));
```

### `Editor::line`

```rust,ignore
pub fn line(&self) -> &str
```

The text of the line being edited.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("1 + 2");
assert_eq!(editor.line(), "1 + 2");
assert!(editor.apply(Edit::Backspace));
assert_eq!(editor.line(), "1 + ");
```

### `Editor::cursor`

```rust,ignore
pub fn cursor(&self) -> usize
```

The cursor position as a byte offset into [`line`](#editorline). It is always
on a `char` boundary, so `&line[..cursor]` and `&line[cursor..]` are always
valid. Use it to find the text to complete; use [`column`](#editorcolumn) to
place the terminal cursor.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("print(len");
let prefix = &editor.line()[..editor.cursor()];
let word = &prefix[prefix.rfind(|c: char| !c.is_alphanumeric()).map_or(0, |i| i + 1)..];
assert_eq!(word, "len"); // the word to complete

assert!(editor.apply(Edit::Home));
assert_eq!(editor.cursor(), 0);
```

### `Editor::column`

```rust,ignore
pub fn column(&self) -> usize
```

The cursor's position in terminal columns from the start of the line: the
display width of the text before the cursor. Wide characters count two columns,
combining marks none. After drawing the prompt and the line, move the terminal
cursor to `prompt_width + column()`. Computed per call in time linear in the
text before the cursor, with a fast path for ASCII.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("x = 世界");
assert_eq!(editor.cursor(), 10); // bytes: 4 ASCII + 2 × 3
assert_eq!(editor.column(), 8);  // columns: 4 ASCII + 2 × 2

let mut accented = Editor::new();
accented.insert("cafe\u{0301}"); // `e` + combining accent
assert_eq!(accented.column(), 4);
assert!(accented.apply(Edit::Left)); // one step over `é`
assert_eq!(accented.column(), 3);
```

### `Editor::apply`

```rust,ignore
pub fn apply(&mut self, edit: Edit) -> bool
```

Applies one editing command and reports whether the line or the cursor changed.

| Parameter | Meaning |
|---|---|
| `edit` | The command; see [`Edit`](#edit) for each one's behaviour. |

**Returns** `false` when the command could do nothing — moving left at the
start, deleting at the end, yanking an empty kill buffer, inserting a control
character — and leaves the editor untouched, so the host can skip a redraw or
ring the bell. Kill commands replace the kill buffer with what they remove; a
kill that removes nothing leaves it as it was.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("hello world");

assert!(editor.apply(Edit::KillWordLeft));
assert_eq!(editor.line(), "hello ");

assert!(editor.apply(Edit::Home));
assert!(editor.apply(Edit::Yank));
assert_eq!(editor.line(), "worldhello ");
assert_eq!(editor.cursor(), 5);

assert!(editor.apply(Edit::Home));
assert!(!editor.apply(Edit::Left));          // already at the start
assert!(!editor.apply(Edit::Insert('\t')));  // control characters are refused
```

Driving it from a keymap:

```rust
use repl_lang::{Edit, Editor};

fn emacs(key: &str) -> Option<Edit> {
    Some(match key {
        "C-a" => Edit::Home,
        "C-e" => Edit::End,
        "C-k" => Edit::KillToEnd,
        "C-y" => Edit::Yank,
        "M-b" => Edit::WordLeft,
        _ => return None,
    })
}

let mut editor = Editor::new();
editor.insert("say hello");
for key in ["M-b", "C-k", "C-a", "C-y"] {
    if let Some(edit) = emacs(key) {
        editor.apply(edit);
    }
}
assert_eq!(editor.line(), "hellosay ");
```

### `Editor::insert`

```rust,ignore
pub fn insert(&mut self, text: &str) -> bool
```

Inserts `text` at the cursor, as a paste would, and returns `true` if anything
was inserted.

| Parameter | Meaning |
|---|---|
| `text` | Text to insert. Control characters (including `'\n'` and `'\t'`) are dropped; everything else is inserted in one pass and the cursor moves past it. |

A host receiving a multi-line paste should split it into lines and submit them
one by one, or feed the whole paste to the [`Session`](#session) directly.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("fn main() {}");
assert!(editor.apply(Edit::Left));
editor.insert(" body ");
assert_eq!(editor.line(), "fn main() { body }");
```

```rust
use repl_lang::Editor;

let mut editor = Editor::new();
assert!(editor.insert("a\tb\u{1b}c")); // tab and escape dropped
assert_eq!(editor.line(), "abc");
assert!(!editor.insert("\r\n"));       // nothing typable
```

### `Editor::clear`

```rust,ignore
pub fn clear(&mut self) -> bool
```

Empties the line and moves the cursor to the start, keeping the kill buffer and
the history; returns `true` if the line was not already empty. This is the
editor half of an interrupt (`Ctrl-C`). If the history was being browsed,
browsing ends and the parked draft is dropped too.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("half-typed");
assert!(editor.apply(Edit::KillWordLeft)); // kill buffer: "typed"
assert!(editor.clear());
assert_eq!(editor.line(), "");
assert!(!editor.clear());
assert!(editor.apply(Edit::Yank));         // the kill buffer survived
assert_eq!(editor.line(), "typed");
```

### `Editor::submit`

```rust,ignore
pub fn submit(&mut self) -> &str
```

Finishes the line: returns its text, records it in the history, and leaves the
editor empty. The line is recorded unless it is blank or identical to the
newest history entry, and only if the capacity is not `0`. Submitting while
browsing the history submits the recalled line as edited and drops the draft.
The returned text stays borrowed from the editor, which recycles the buffer for
the next line.

```rust
use repl_lang::Editor;

let mut editor = Editor::new();
editor.insert("1 + 1");
assert_eq!(editor.submit(), "1 + 1");

editor.insert("   ");             // blank: returned but not recorded
assert_eq!(editor.submit(), "   ");
editor.insert("1 + 1");           // repeat: not recorded twice
editor.submit();
assert!(editor.history().eq(["1 + 1"]));
```

Feeding a session straight from the editor:

```rust
use repl_lang::{Editor, Feed, Session, Status};

let mut editor = Editor::new();
let mut session = Session::new();
editor.insert("2 + 2");
let feed = session.feed(editor.submit(), |input| Status::Complete(input.text().len()))?;
assert!(matches!(feed, Feed::Complete { value: 6, .. }));
# Ok::<(), repl_lang::SessionError>(())
```

### `Editor::add_history`

```rust,ignore
pub fn add_history(&mut self, line: &str)
```

Records `line` in the history as if it had been submitted, without touching
the line being edited. Use it to restore history saved by an earlier session.

| Parameter | Meaning |
|---|---|
| `line` | The text to record. Control characters are removed first; the result is skipped if blank or equal to the newest entry; the oldest entry is dropped once the history is full. |

```rust
use repl_lang::{Edit, Editor};

let saved = "let x = 1\nx * 2\n";
let mut editor = Editor::new();
for line in saved.lines() {
    editor.add_history(line);
}
assert!(editor.apply(Edit::HistoryPrev));
assert_eq!(editor.line(), "x * 2");
```

```rust
use repl_lang::Editor;

let mut editor = Editor::new();
editor.add_history("ls\t-la");  // the tab is removed
editor.add_history("\u{7}");    // nothing left: skipped
assert!(editor.history().eq(["ls-la"]));
```

### `Editor::history`

```rust,ignore
pub fn history(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator + '_
```

The history, oldest entry first. Reverse it for newest-first, or write it out
line by line to persist it between sessions.

```rust
use repl_lang::Editor;

let mut editor = Editor::new();
for line in ["first", "second", "third"] {
    editor.insert(line);
    editor.submit();
}
assert_eq!(editor.history().len(), 3);
assert_eq!(editor.history().next_back(), Some("third"));
let newest_first: Vec<&str> = editor.history().rev().collect();
assert_eq!(newest_first, ["third", "second", "first"]);

// Persisting and restoring.
let saved: String = editor.history().flat_map(|l| [l, "\n"]).collect();
let mut restored = Editor::new();
saved.lines().for_each(|l| restored.add_history(l));
assert!(restored.history().eq(editor.history()));
```

---

## `Edit`

```rust,ignore
#[non_exhaustive]
pub enum Edit {
    Insert(char),
    Backspace,
    Delete,
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
    KillToStart,
    KillToEnd,
    KillWordLeft,
    KillWordRight,
    Yank,
    HistoryPrev,
    HistoryNext,
}
```

One editing command for an [`Editor`](#editor) — the boundary between the
terminal and the editor. Motions and deletions work in
[display units](#display-units-and-words). The readline bindings in the table
are a guide; none are built in.

| Variant | Effect | Usual keys |
|---|---|---|
| `Insert(c)` | Inserts `c` at the cursor; refused for control characters. | printable keys |
| `Backspace` | Deletes the unit before the cursor. | `Backspace`, `C-h` |
| `Delete` | Deletes the unit under the cursor. | `Delete`, `C-d` |
| `Left` / `Right` | Moves one unit. | `←` `→`, `C-b` `C-f` |
| `WordLeft` | Moves to the start of the current or previous word. | `C-←`, `M-b` |
| `WordRight` | Moves to the end of the current or next word. | `C-→`, `M-f` |
| `Home` / `End` | Moves to the start or end of the line. | `Home` `End`, `C-a` `C-e` |
| `KillToStart` | Cuts everything before the cursor. | `C-u` |
| `KillToEnd` | Cuts from the cursor to the end. | `C-k` |
| `KillWordLeft` | Cuts the word before the cursor. | `C-w`, `M-Backspace` |
| `KillWordRight` | Cuts the word after the cursor. | `M-d` |
| `Yank` | Inserts the kill buffer at the cursor. | `C-y` |
| `HistoryPrev` | Recalls the previous (older) entry, parking the draft first. | `↑`, `C-p` |
| `HistoryNext` | Recalls the next (newer) entry, or restores the draft. | `↓`, `C-n` |

`Edit` implements `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, and `Hash`. It
is `#[non_exhaustive]` so commands can be added in a minor release; hosts
construct `Edit` values rather than match on them, so this costs nothing in
practice.

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
for c in "print x".chars() {
    editor.apply(Edit::Insert(c));
}
assert!(editor.apply(Edit::WordLeft));    // cursor before `x`
assert!(editor.apply(Edit::KillToStart)); // cuts "print "
assert_eq!(editor.line(), "x");
assert!(editor.apply(Edit::Yank));        // and puts it back
assert_eq!(editor.line(), "print x");
```

Word motion over an expression:

```rust
use repl_lang::{Edit, Editor};

let mut editor = Editor::new();
editor.insert("total_cost = price * (1 + tax_rate)");
let mut stops = Vec::new();
while editor.apply(Edit::WordLeft) {
    stops.push(editor.cursor());
}
assert_eq!(stops, [26, 22, 13, 0]); // tax_rate, 1, price, total_cost
```

---

## Re-exports

So that the types in this crate's signatures can be named without adding the
family crates as direct dependencies, repl-lang re-exports:

| Item | From | Appears in |
|---|---|---|
| `Cursor` | `lexer_lang` | [`Input::cursor`](#inputcursor) |
| `Diagnostic` | `diag_lang` | [`Input::is_incomplete`](#inputis_incomplete) |
| `SourceMap` | `source_lang` | [`Session::sources`](#sessionsources) |
| `SourceFile` | `source_lang` | [`Session::entry`](#sessionentry) |
| `SourceId` | `source_lang` | [`Entry::id`](#entry) |
| `Span`, `BytePos` | `source_lang` (from `span_lang`) | [`Input::span`](#inputspan), [`Input::base`](#inputbase), [`Entry::span`](#entry) |

To *build* or *render* diagnostics, depend on `diag-lang` itself for
`Label`, `Severity`, and `Renderer`.

```rust
use repl_lang::{BytePos, Diagnostic, SourceMap, Span};

let span = Span::new(BytePos::new(2).to_u32(), 5);
assert_eq!(span.len(), 3);
let map = SourceMap::new();
assert!(map.is_empty());
let _: Option<Diagnostic> = None;
```

---

## Feature flags

| Feature | Default | Effect |
|---|---|---|
| `std` | yes | The standard library. Without it the crate is `#![no_std]` and needs only `alloc`; neither the session nor the editor uses operating-system facilities. Forwards to `std` on every dependency. |

```toml
[dependencies]
repl-lang = { version = "1", default-features = false }
```

---

## Guide: wiring a parser

A complete pipeline with a real lexer and [`parser-lang`](https://crates.io/crates/parser-lang):
arithmetic over `+`, `*`, and parentheses. The lexer runs on
[`Input::cursor`](#inputcursor), so token spans are global; the parser reports
a missing operand at the end of input, so
[`Input::is_incomplete`](#inputis_incomplete) turns that into continuation.

```rust
use diag_lang::Diagnostic;
use parser_lang::{Parser, Pratt, Span, Token, TokenKind};
use repl_lang::{Feed, Input, Session, Status};

#[derive(Clone, Copy, PartialEq, Debug)]
enum K { Num(i64), Plus, Star, LParen, RParen, Space, Bad, Eof }

impl TokenKind for K {
    fn is_trivia(&self) -> bool { matches!(self, K::Space) }
    fn is_eof(&self) -> bool { matches!(self, K::Eof) }
}

fn lex(input: Input<'_>) -> Vec<Token<K>> {
    let mut cursor = input.cursor();
    let mut tokens = Vec::new();
    while let Some(c) = cursor.bump() {
        let kind = match c {
            '0'..='9' => {
                cursor.eat_while(|c| c.is_ascii_digit());
                K::Num(cursor.lexeme().parse().unwrap_or(i64::MAX))
            }
            '+' => K::Plus,
            '*' => K::Star,
            '(' => K::LParen,
            ')' => K::RParen,
            c if c.is_whitespace() => K::Space,
            _ => K::Bad,
        };
        tokens.push(cursor.emit(kind));
    }
    tokens.push(Token::new(K::Eof, Span::empty(cursor.pos().to_u32())));
    tokens
}

struct Arith;

impl<'t> Pratt<'t, K> for Arith {
    type Output = i64;

    fn prefix(&mut self, p: &mut Parser<'t, K>) -> Option<i64> {
        if p.at_end() {
            p.error("expected a number");
            return None;
        }
        let token = p.bump()?;
        match *token.kind() {
            K::Num(n) => Some(n),
            K::LParen => {
                let inner = self.expression(p, 0)?;
                p.expect(|k| *k == K::RParen, "`)`")?;
                Some(inner)
            }
            _ => {
                p.error_at(token.span(), "expected a number");
                None
            }
        }
    }

    fn infix_binding(&self, kind: &K) -> Option<(u8, u8)> {
        match kind {
            K::Plus => Some((1, 2)),
            K::Star => Some((3, 4)),
            _ => None,
        }
    }

    fn infix(&mut self, op: &'t Token<K>, l: i64, r: i64) -> Option<i64> {
        Some(if *op.kind() == K::Plus { l.saturating_add(r) } else { l.saturating_mul(r) })
    }
}

/// The pipeline: lex, parse, and decide.
fn pipeline(input: Input<'_>) -> Status<Result<i64, Vec<Diagnostic>>> {
    let tokens = lex(input);
    let mut parser = Parser::new(&tokens);
    let value = Arith.parse(&mut parser);
    if value.is_some() && !parser.at_end() {
        parser.error("expected an operator");
    }
    let errors = parser.into_errors();
    if input.is_incomplete(&errors) {
        return Status::Incomplete;
    }
    Status::Complete(match value {
        Some(v) if errors.is_empty() => Ok(v),
        _ => Err(errors),
    })
}

let mut session = Session::new();

// `(2 +` ends inside a group: the parser's error is at the end, so wait.
assert_eq!(session.feed("(2 +", pipeline)?, Feed::Incomplete);
assert_eq!(session.feed("  3) *", pipeline)?, Feed::Incomplete);
assert!(matches!(session.feed("4", pipeline)?, Feed::Complete { value: Ok(20), .. }));

// `1 + * 2` is wrong at the `*`: no amount of typing fixes it, so complete.
let Feed::Complete { value: Err(errors), .. } = session.feed("1 + * 2", pipeline)? else {
    return Err("expected errors".into());
};
let text = diag_lang::Renderer::new().render(&errors[0], session.sources());
assert!(text.contains("<repl:2>:1:5"), "{text}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

The `calc` example in the repository extends this with `let` bindings,
checked arithmetic, comments, and diagnostics that reach back into earlier
entries.

---

## Guide: driving the editor from a terminal

A terminal host has three jobs the editor leaves to it: reading key events,
mapping them to [`Edit`](#edit) commands, and drawing. The mapping is a plain
function; the sketch below uses a local `Key` type standing in for a terminal
library's event type (crossterm's `KeyEvent`, termion's `Key`, and so on).

```rust
use repl_lang::{Edit, Editor, Feed, Session, Status};

/// Stand-in for a terminal library's key event.
#[derive(Clone, Copy)]
enum Key { Char(char), Ctrl(char), Alt(char), Backspace, Left, Right, Up, Down, Enter }

/// The keymap: readline-style bindings.
fn edit_for(key: Key) -> Option<Edit> {
    Some(match key {
        Key::Char(c) => Edit::Insert(c),
        Key::Backspace | Key::Ctrl('h') => Edit::Backspace,
        Key::Left | Key::Ctrl('b') => Edit::Left,
        Key::Right | Key::Ctrl('f') => Edit::Right,
        Key::Up | Key::Ctrl('p') => Edit::HistoryPrev,
        Key::Down | Key::Ctrl('n') => Edit::HistoryNext,
        Key::Ctrl('a') => Edit::Home,
        Key::Ctrl('e') => Edit::End,
        Key::Ctrl('k') => Edit::KillToEnd,
        Key::Ctrl('u') => Edit::KillToStart,
        Key::Ctrl('w') => Edit::KillWordLeft,
        Key::Ctrl('y') => Edit::Yank,
        Key::Alt('b') => Edit::WordLeft,
        Key::Alt('f') => Edit::WordRight,
        Key::Alt('d') => Edit::KillWordRight,
        _ => return None,
    })
}

/// What a real host would write to the terminal: clear the row, print the
/// prompt and the line, then move the cursor to the right column.
fn draw(prompt: &str, editor: &Editor) -> String {
    format!("\r\x1b[2K{prompt}{}\r\x1b[{}C", editor.line(), prompt.len() + editor.column())
}

let mut editor = Editor::new();
let mut session = Session::new();
let mut screen = Vec::new();
let mut results = Vec::new();

let keys = "sum(1,".chars().map(Key::Char)
    .chain([Key::Enter])
    .chain("2)".chars().map(Key::Char))
    .chain([Key::Enter]);

for key in keys {
    let prompt = if session.is_pending() { "... " } else { ">>> " };
    if let Key::Enter = key {
        let line = editor.submit();
        let feed = session.feed(line, |input| {
            if input.text().matches('(').count() > input.text().matches(')').count() {
                Status::Incomplete
            } else {
                Status::Complete(input.text().replace('\n', " ").trim_end().to_owned())
            }
        })?;
        if let Feed::Complete { value, .. } = feed {
            results.push(value);
        }
    } else if let Some(edit) = edit_for(key) {
        if editor.apply(edit) {
            screen.push(draw(prompt, &editor));
        }
    }
}

assert_eq!(results, ["sum(1, 2)"]);
assert_eq!(screen.last().map(String::as_str), Some("\r\x1b[2K... 2)\r\x1b[6C"));
# Ok::<(), repl_lang::SessionError>(())
```

Two practical notes. Put the terminal in raw mode only while reading a line,
and restore it before printing results, so output and panics render normally.
And on `Ctrl-C`, call both [`Editor::clear`](#editorclear) and
[`Session::cancel`](#sessioncancel): the first drops the line being typed, the
second the entry it was continuing.

---

## Stability

repl-lang `1.0.0` freezes the public API. It follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html): nothing below
changes in a breaking way before `2.0`. Additions — new methods, new `Edit`
commands, new `SessionError` variants, new optional features — arrive in minor
releases.

### The frozen surface

| Item | Frozen as |
|---|---|
| [`Session`](#session) | `DEFAULT_LIMIT`, `new`, `with_limit`, `feed`, `cancel`, `is_pending`, `pending`, `len`, `is_empty`, `entry`, `sources`, `limit`; `Clone`, `Debug`, `Default`, `Send`, `Sync`. |
| [`Input`](#input) | `text`, `base`, `span`, `number`, `cursor`, `is_incomplete`; `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`. |
| [`Status`](#status) | Exactly `Complete(T)` and `Incomplete` — an exhaustive enum. |
| [`Feed`](#feed) | Exactly `Complete { entry, value }`, `Incomplete`, and `Empty` — an exhaustive enum. |
| [`Entry`](#entry) | `number`, `id`, `span`; `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Hash`. |
| [`SessionError`](#sessionerror) | `TooLong { len, limit }`, `SpaceExhausted { needed, available }`; `#[non_exhaustive]`; `Display` and `core::error::Error`. |
| [`Editor`](#editor) | `DEFAULT_HISTORY`, `new`, `with_history`, `line`, `cursor`, `column`, `apply`, `insert`, `clear`, `submit`, `add_history`, `history`; `Clone`, `Debug`, `Default`, `Send`, `Sync`. |
| [`Edit`](#edit) | The sixteen commands listed above; `#[non_exhaustive]`. |
| [Re-exports](#re-exports) | `Cursor`, `Diagnostic`, `SourceMap`, `SourceFile`, `SourceId`, `Span`, `BytePos`, from the `1.x` / `0.4.x` lines of their crates. |

`Status` and `Feed` are exhaustive on purpose. A host matches `Feed` in every
read loop, and the set is closed by design: an entry is finished, unfinished,
or there was nothing to evaluate. A new pipeline verdict would need a new
`Feed` outcome, which is a `2.0` change.

### Behaviour that is part of the contract

- **Entry layout.** Committed entries are sources named `<repl:N>`, `N`
  counting from `1`, laid out end to end in the session's source map in commit
  order; [`Input::base`](#inputbase) is exactly where the entry lands.
- **Feeding.** One trailing `"\n"` or `"\r\n"` is removed and a `'\n'`
  appended; a blank line with nothing pending returns `Feed::Empty` without
  calling the pipeline or using a number; blank lines inside an entry reach the
  pipeline; a refused line discards the pending entry without calling the
  pipeline.
- **Completeness.** [`Input::is_incomplete`](#inputis_incomplete) is `true`
  exactly when there is at least one `Severity::Error` diagnostic and every one
  starts at or after the last non-whitespace byte of the input.
- **Limits.** `Session::DEFAULT_LIMIT` is 1 MiB and counts line terminators;
  `Editor::DEFAULT_HISTORY` is 1000.
- **Editing.** Display units (a visible character plus following zero-width
  characters), words (`XID_Continue` runs), control-character refusal on every
  path into the line, the history recording rules (no blanks, no immediate
  repeats, oldest dropped at capacity), and the draft rules (restored by
  `HistoryNext`, dropped by `submit` and `clear`).
- **MSRV.** Rust 1.85. A rise is a minor-version change, never a patch.

### Not part of the contract

- The wording of `SessionError`'s `Display` output.
- `Debug` output of any type.
- Display widths and identifier classes for characters whose Unicode
  properties change in a future Unicode version; they follow `unicode-lang`.
- Performance characteristics, although regressions are treated as bugs.
