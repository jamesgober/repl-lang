//! A session driving a real pipeline: the calculator from the examples, lexed
//! with `Input::cursor` and parsed with `parser_lang`.

#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "../examples/common/mod.rs"]
mod common;

use common::{Calc, Reply};
use diag_lang::{Diagnostic, Renderer};
use repl_lang::{Feed, Session, Span};

type Outcome = Result<Reply, Vec<Diagnostic>>;

/// Feeds `lines` and returns every outcome, `None` for each incomplete line.
fn run(session: &mut Session, calc: &mut Calc, lines: &[&str]) -> Vec<Option<Outcome>> {
    lines
        .iter()
        .map(
            |line| match session.feed(line, |input| calc.run(input)).unwrap() {
                Feed::Complete { value, .. } => Some(value),
                Feed::Incomplete => None,
                Feed::Empty => panic!("unexpected blank line in {lines:?}"),
            },
        )
        .collect()
}

fn value(outcome: &Option<Outcome>) -> i64 {
    match outcome {
        Some(Ok(Reply::Value(n) | Reply::Bound { value: n, .. })) => *n,
        other => panic!("expected a value, got {other:?}"),
    }
}

fn render(session: &Session, outcome: &Option<Outcome>) -> String {
    let Some(Err(diagnostics)) = outcome else {
        panic!("expected diagnostics, got {outcome:?}");
    };
    let renderer = Renderer::new();
    diagnostics
        .iter()
        .map(|d| renderer.render(d, session.sources()))
        .collect()
}

#[test]
fn test_single_line_entry_evaluates() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["1 + 2 * 3"]);
    assert_eq!(value(&out[0]), 7);
}

#[test]
fn test_open_paren_continues_until_closed() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["(1 +", "2", ") * 4"]);
    assert!(out[0].is_none());
    assert!(out[1].is_none());
    assert_eq!(value(&out[2]), 12);
    assert_eq!(session.len(), 1);
    assert_eq!(session.entry(1).unwrap().text(), "(1 +\n2\n) * 4\n");
}

#[test]
fn test_trailing_operator_and_open_let_continue() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["let x =", "  10 -", "  4"]);
    assert!(out[0].is_none() && out[1].is_none());
    assert_eq!(
        out[2],
        Some(Ok(Reply::Bound {
            name: "x".into(),
            value: 6
        }))
    );
}

#[test]
fn test_trailing_comment_does_not_hide_open_operator() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["2 *   # double it", "21"]);
    assert!(out[0].is_none());
    assert_eq!(value(&out[1]), 42);
}

#[test]
fn test_error_mid_line_completes_instead_of_waiting() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    for wrong in ["1 + ) 2", "1 2", "(1 +", "1 + @"] {
        let out = session.feed(wrong, |input| calc.run(input)).unwrap();
        if wrong == "(1 +" {
            assert_eq!(out, Feed::Incomplete);
            assert!(session.cancel());
        } else {
            assert!(
                matches!(out, Feed::Complete { value: Err(_), .. }),
                "{wrong:?} gave {out:?}"
            );
        }
    }
}

#[test]
fn test_error_on_second_line_renders_its_own_line_number() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["(1 +", "  nope)"]);
    let text = render(&session, &out[1]);
    assert!(text.contains("cannot find `nope`"), "{text}");
    assert!(text.contains("<repl:1>:2:3"), "{text}");
}

#[test]
fn test_diagnostic_points_into_earlier_entry() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(&mut session, &mut calc, &["let n = 1", "2", "let n = 3"]);
    let text = render(&session, &out[2]);
    assert!(text.contains("<repl:3>:1:5"), "{text}");
    assert!(text.contains("<repl:1>:1:5"), "{text}");
    assert!(text.contains("first defined here"), "{text}");
}

#[test]
fn test_bindings_persist_across_entries() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(
        &mut session,
        &mut calc,
        &["let a = 2", "let b = a ^ 10", "b - a"],
    );
    assert_eq!(value(&out[2]), 1022);
}

#[test]
fn test_entry_span_matches_pipeline_input_span() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    run(&mut session, &mut calc, &["let a = 1"]);
    let mut seen = Span::empty(0);
    let feed = session
        .feed("a + 1", |input| {
            seen = input.span();
            calc.run(input)
        })
        .unwrap();
    let Feed::Complete { entry, .. } = feed else {
        panic!("{feed:?}")
    };
    assert_eq!(entry.span(), seen);
    assert_eq!(entry.span(), Span::new(10, 16));
}

#[test]
fn test_overflow_and_division_by_zero_are_diagnostics() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(
        &mut session,
        &mut calc,
        &["9223372036854775807 + 1", "1 / (2 - 2)", "-(2 ^ 63)"],
    );
    assert!(render(&session, &out[0]).contains("arithmetic overflow"));
    assert!(render(&session, &out[1]).contains("division by zero"));
    assert!(render(&session, &out[2]).contains("arithmetic overflow"));
}

#[test]
fn test_precedence_and_associativity() {
    let (mut session, mut calc) = (Session::new(), Calc::new());
    let out = run(
        &mut session,
        &mut calc,
        &["2 ^ 3 ^ 2", "-2 ^ 2", "10 - 4 - 3", "7 % 4 * 2"],
    );
    let values: Vec<i64> = out.iter().map(value).collect();
    assert_eq!(values, [512, -4, 3, 6]);
}
