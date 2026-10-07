//! # repl_lang
//!
//! The parts of a read-eval-print loop that every language needs and none
//! should have to write twice: deciding when typed input forms a complete
//! entry, keeping the session's entries so diagnostics can point back into
//! them, and editing the line being typed.
//!
//! The crate owns no syntax and does no I/O. A language plugs in its own
//! *pipeline* — lex, parse, evaluate — as a closure, and its own terminal or
//! line source. repl-lang supplies the loop state between them:
//!
//! - [`Session`] accumulates lines into an entry, runs the pipeline on it after
//!   each line, and commits the entry once the pipeline reports
//!   [`Status::Complete`]. Every committed entry becomes a source named
//!   `<repl:N>` in one [`SourceMap`], so a `diag_lang` renderer can show any
//!   diagnostic from any entry.
//! - [`Input`] is what the pipeline sees: the entry so far, its global position,
//!   a [`lexer_lang::Cursor`] already placed there, and
//!   [`is_incomplete`](Input::is_incomplete) — the "errors only at the end of
//!   input mean *keep typing*" test that turns any error-reporting parser into
//!   a continuation-line detector.
//! - [`Editor`] is a terminal-agnostic line editor: cursor motion by visible
//!   character and by word, kill and yank, and a bounded history, driven by
//!   [`Edit`] commands the host maps from its own key events.
//!
//! ## Example
//!
//! A session over a fixed script, with a pipeline that asks for more input
//! until brackets balance:
//!
//! ```
//! use repl_lang::{Feed, Session, Status};
//!
//! let mut session = Session::new();
//! let mut prompts = Vec::new();
//!
//! for line in ["sum([1, 2,", "    3, 4])", "len([])"] {
//!     prompts.push(if session.is_pending() { "..." } else { ">>>" });
//!     let feed = session.feed(line, |input| {
//!         let open = input.text().matches('[').count() + input.text().matches('(').count();
//!         let close = input.text().matches(']').count() + input.text().matches(')').count();
//!         if open > close { Status::Incomplete } else { Status::Complete(input.text().len()) }
//!     })?;
//!     if let Feed::Complete { entry, value } = feed {
//!         println!("entry {} complete: {value} bytes", entry.number());
//!     }
//! }
//!
//! assert_eq!(prompts, [">>>", "...", ">>>"]);
//! assert_eq!(session.len(), 2);
//! assert_eq!(session.entry(1).map(|e| e.name()), Some("<repl:1>"));
//! # Ok::<(), repl_lang::SessionError>(())
//! ```
//!
//! ## Wiring a parser
//!
//! With a parser that reports a missing token at the end of input — as
//! [`parser_lang`](https://docs.rs/parser-lang) does — the whole completeness
//! check is one call:
//!
//! ```text
//! session.feed(line, |input| {
//!     let tokens = lex(input.cursor());          // spans are already global
//!     let mut parser = Parser::new(&tokens);
//!     let program = parse_program(&mut parser);
//!     let errors = parser.into_errors();
//!     if input.is_incomplete(&errors) {
//!         Status::Incomplete                      // "1 + (2 *" — keep reading
//!     } else {
//!         Status::Complete((program, errors))     // done, right or wrong
//!     }
//! })
//! ```
//!
//! The `calc` example in the repository is a complete REPL built this way.
//!
//! ## Features
//!
//! - `std` (default) — the standard library. Without it the crate is
//!   `#![no_std]` and needs only `alloc`; neither the session nor the editor
//!   uses operating-system facilities.
//!
//! ## Stability
//!
//! The public surface is frozen and stable as of `1.0.0`: it follows Semantic
//! Versioning, with no breaking changes before `2.0`. The surface, the
//! behaviour that is part of the contract — entry naming and layout, the
//! completeness rule, the history and display-unit rules — and the SemVer
//! promise are catalogued in
//! [`docs/API.md`](https://github.com/jamesgober/repl-lang/blob/main/docs/API.md#stability).

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(
    warnings,
    missing_docs,
    unsafe_op_in_unsafe_fn,
    unused_must_use,
    unused_results,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::undocumented_unsafe_blocks
)]

extern crate alloc;

mod edit;
mod editor;
mod error;
mod feed;
mod input;
mod session;
mod text;

pub use edit::Edit;
pub use editor::Editor;
pub use error::SessionError;
pub use feed::{Entry, Feed, Status};
pub use input::Input;
pub use session::Session;

// Re-exported so the types in this crate's signatures can be named without
// depending on the family crates directly.
pub use diag_lang::Diagnostic;
pub use lexer_lang::Cursor;
pub use source_lang::{BytePos, SourceFile, SourceId, SourceMap, Span};

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;
