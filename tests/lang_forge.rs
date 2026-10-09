//! A session driving a parser that reports positions relative to the entry
//! text rather than the session's global positions: a language forged with
//! `lang-forge`, which parses `Input::text` on its own. Such a pipeline uses
//! `Input::is_incomplete_relative`, which must judge every entry the same way,
//! however far into the session's position space it lands (ISSUES M72:
//! `is_incomplete`, the method for global spans, judges these entries
//! complete from the second one on).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lang_forge::Language;
use repl_lang::{Feed, Input, Session, Status};

/// A small statement language: every statement ends with `;`.
fn calc() -> Language {
    Language::from_lsf(
        r##"
        [language]
        name = "calc"

        [rules]
        program = "stmt*"
        stmt    = "'let' IDENT '=' expr ';' | expr ';'"
        group   = "'(' expr ')'"

        [rules.expr]
        operand = "NUMBER | IDENT | group"
        levels  = [
            { left = ["+", "-"] },
            { left = ["*", "/"] },
        ]
        "##,
    )
    .expect("the schematic forges")
}

/// What a finished entry produced: its text and whether it had errors.
#[derive(Debug, PartialEq, Eq)]
struct Done {
    text: String,
    errors: bool,
}

/// The pipeline: parse the entry text alone (relative spans) and let
/// `is_incomplete_relative` decide.
fn pipeline(language: &Language, input: Input<'_>) -> Status<Done> {
    let parse = language.parse(input.text());
    if input.is_incomplete_relative(parse.diagnostics()) {
        Status::Incomplete
    } else {
        Status::Complete(Done {
            text: input.text().to_owned(),
            errors: parse.has_errors(),
        })
    }
}

/// Feeds `lines`, returning `None` for each line that left the entry
/// unfinished and the result for each that finished one.
fn run(session: &mut Session, language: &Language, lines: &[&str]) -> Vec<Option<Done>> {
    lines
        .iter()
        .map(|line| {
            match session
                .feed(line, |input| pipeline(language, input))
                .unwrap()
            {
                Feed::Complete { value, .. } => Some(value),
                Feed::Incomplete => None,
                Feed::Empty => panic!("unexpected blank line {line:?}"),
            }
        })
        .collect()
}

fn done(text: &str, errors: bool) -> Option<Done> {
    Some(Done {
        text: text.to_owned(),
        errors,
    })
}

#[test]
fn test_relative_spans_continue_unfinished_entries_in_every_entry() {
    let language = calc();
    let mut session = Session::new();
    let out = run(
        &mut session,
        &language,
        &[
            "let a = 1;", // entry 1, base 0
            "let b = (2 +",
            "3);", // entry 2, base 11
            "a *",
            "b",
            ";",                                    // entry 3, base 28: shorter than its base
            "let total = (a + b) * (a - b) + (1 +", // entry 4: longer than its base
            "2);",
        ],
    );
    assert_eq!(
        out,
        [
            done("let a = 1;\n", false),
            None,
            done("let b = (2 +\n3);\n", false),
            None,
            None,
            done("a *\nb\n;\n", false),
            None,
            done("let total = (a + b) * (a - b) + (1 +\n2);\n", false),
        ]
    );
    assert_eq!(session.len(), 4);
}

#[test]
fn test_relative_spans_report_mid_entry_errors_at_once() {
    let language = calc();
    let mut session = Session::new();
    let out = run(
        &mut session,
        &language,
        &[
            "let a = 1;",
            "let b = 1 + ) ;", // wrong, not unfinished
            "let c = (1 +",    // unfinished
            "2;",              // `)` is missing mid-entry: wrong
            "a + ",            // unfinished again
            "1;",
        ],
    );
    assert_eq!(
        out,
        [
            done("let a = 1;\n", false),
            done("let b = 1 + ) ;\n", true),
            None,
            done("let c = (1 +\n2;\n", true),
            None,
            done("a + \n1;\n", false),
        ]
    );
}

#[test]
fn test_relative_and_global_methods_agree_on_every_entry() {
    // The same parse judged twice: its relative spans by
    // `is_incomplete_relative`, and the same spans shifted to global positions
    // by `is_incomplete`. The verdicts must agree on every line of a long
    // session.
    let language = calc();
    let mut session = Session::new();
    let lines = [
        "let x = 10;",
        "x * (2 +",
        "3)",
        ";",
        "let y = x -",
        "1;",
        "y +",
        "x;",
    ];
    for line in lines {
        let feed = session
            .feed(line, |input| {
                let parse = language.parse(input.text());
                let relative = input.is_incomplete_relative(parse.diagnostics());
                let shifted: Vec<_> = parse
                    .diagnostics()
                    .iter()
                    .map(|d| {
                        let span = d.primary().span();
                        let base = input.base().to_u32();
                        let global = repl_lang::Span::new(
                            span.start().to_u32() + base,
                            span.end().to_u32() + base,
                        );
                        diag_lang::Diagnostic::new(
                            d.severity(),
                            d.message(),
                            diag_lang::Label::new(global, ""),
                        )
                    })
                    .collect();
                assert_eq!(
                    input.is_incomplete(&shifted),
                    relative,
                    "entry {} line {line:?}",
                    input.number()
                );
                if relative {
                    Status::Incomplete
                } else {
                    Status::Complete(())
                }
            })
            .unwrap();
        assert!(!matches!(feed, Feed::Empty));
    }
    assert_eq!(session.len(), 4);
}

#[test]
fn test_global_method_misjudges_relative_spans_after_the_first_entry() {
    // Why the relative method exists: `is_incomplete` reads spans as global,
    // so the forged parser's end-of-input error in entry 2 (relative 13, the
    // end of "let b = (2 +") falls before the entry's global end (11 + 12).
    let language = calc();
    let mut session = Session::new();
    session
        .feed("let a = 1;", |_| Status::Complete(()))
        .unwrap();
    session
        .feed("let b = (2 +", |input| {
            let parse = language.parse(input.text());
            assert!(parse.has_errors());
            assert!(input.is_incomplete_relative(parse.diagnostics()));
            assert!(!input.is_incomplete(parse.diagnostics()));
            Status::Complete(())
        })
        .unwrap();
}
