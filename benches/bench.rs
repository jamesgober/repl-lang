//! Criterion benchmarks for the session feed loop and the line editor.
//!
//! Run with `cargo bench --bench bench`. Reports land in `target/criterion/`.

#![allow(clippy::unwrap_used)]

use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use diag_lang::{Diagnostic, Label, Severity};
use repl_lang::{Edit, Editor, Input, Session, Span, Status};

/// A pipeline that waits for balanced parentheses: the typical cheap
/// completeness check, and a stand-in for "lex and parse".
fn brackets(input: Input<'_>) -> Status<usize> {
    let mut depth = 0_i32;
    for byte in input.text().bytes() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
    }
    if depth > 0 {
        Status::Incomplete
    } else {
        Status::Complete(input.text().len())
    }
}

fn session(c: &mut Criterion) {
    let mut group = c.benchmark_group("session");

    // One hundred single-line entries into a fresh session: the per-entry cost
    // of feeding, the pipeline call, and committing into the source map.
    group.throughput(Throughput::Elements(100));
    group.bench_function("entries/100", |b| {
        b.iter_batched_ref(
            Session::new,
            |session| {
                for _ in 0..100 {
                    black_box(
                        session
                            .feed("let total = price * (1 + tax)", brackets)
                            .unwrap(),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    // One entry spread over `lines` lines: every line but the last opens a
    // parenthesis, the last closes them all. The pipeline re-reads the whole
    // entry on each line, so the cost grows quadratically by design.
    for lines in [2_usize, 8, 64] {
        let mut script = vec![String::from("(define (f x)"); lines - 1];
        script.push(")".repeat(lines - 1));
        group.throughput(Throughput::Elements(lines as u64));
        group.bench_function(format!("continuation/lines={lines}"), |b| {
            b.iter_batched_ref(
                Session::new,
                |session| {
                    for line in &script {
                        black_box(session.feed(line, brackets).unwrap());
                    }
                    debug_assert_eq!(session.len(), 1);
                },
                BatchSize::SmallInput,
            );
        });
    }

    // Feeding a line whose pipeline runs the completeness test on four parser
    // diagnostics, then interrupting: no entry is committed, so the session
    // stays the same size however many iterations run.
    let errors: Vec<Diagnostic> = (0..4)
        .map(|i| {
            Diagnostic::new(
                Severity::Error,
                "expected expression",
                Label::new(Span::empty(30 + i), ""),
            )
        })
        .collect();
    group.throughput(Throughput::Elements(1));
    group.bench_function("feed_incomplete/errors=4", |b| {
        let mut session = Session::new();
        b.iter(|| {
            let feed = session.feed("let total = price * (1 + tax) +   ", |input| {
                if input.is_incomplete(black_box(&errors)) {
                    Status::<()>::Incomplete
                } else {
                    Status::Complete(())
                }
            });
            black_box(feed.unwrap());
            session.cancel()
        });
    });

    group.finish();
}

fn editor(c: &mut Criterion) {
    let mut group = c.benchmark_group("editor");

    // Typing an 80-column line one key at a time into a reused editor.
    let line: String =
        "let total = price * (1 + tax_rate) - discount # compute the final amount here"
            .chars()
            .take(80)
            .collect();
    group.throughput(Throughput::Elements(line.chars().count() as u64));
    group.bench_function("type/80", |b| {
        let mut editor = Editor::new();
        b.iter(|| {
            editor.clear();
            for c in line.chars() {
                editor.apply(Edit::Insert(black_box(c)));
            }
            black_box(editor.cursor())
        });
    });

    // Word motion across a long line, both directions.
    let long = "alpha_beta + gamma(delta, epsilon) * 世界 ".repeat(20);
    group.throughput(Throughput::Bytes(long.len() as u64 * 2));
    group.bench_function("word_motion/line=800", |b| {
        let mut editor = Editor::new();
        editor.insert(&long);
        b.iter(|| {
            while editor.apply(Edit::WordLeft) {}
            while editor.apply(Edit::WordRight) {}
            black_box(editor.cursor())
        });
    });

    // Submitting into a full history ring: the steady state of a long session,
    // recycling the oldest slot instead of allocating.
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit/history_full", |b| {
        let mut editor = Editor::with_history(100);
        for i in 0..100 {
            editor.add_history(&format!("entry number {i}"));
        }
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            editor.insert(if flip {
                "let x = compute(1, 2, 3)"
            } else {
                "let y = compute(4, 5, 6)"
            });
            black_box(editor.submit().len())
        });
    });

    // Display column of the cursor at the end of a long mixed-width line.
    let wide = "abc 世界 e\u{0301} 😀 ".repeat(50);
    group.throughput(Throughput::Bytes(wide.len() as u64));
    group.bench_function("column/line=900", |b| {
        let mut editor = Editor::new();
        editor.insert(&wide);
        b.iter(|| black_box(editor.column()));
    });

    group.finish();
}

criterion_group!(benches, session, editor);
criterion_main!(benches);
