//! A scripted session printed as a notebook-style transcript.
//!
//! Feeds a fixed sequence of lines through a `Session` and the calculator
//! pipeline, numbering cells from the entry numbers the session assigns. It
//! shows the parts of the session that an interactive run makes easy to miss:
//!
//! - entries that span several lines, joined under one cell number;
//! - blank lines, which never consume a number;
//! - an interrupt (`Session::cancel`) abandoning a half-typed entry;
//! - a diagnostic in one cell pointing at a definition in an earlier cell —
//!   possible because every entry lives in the same source map;
//! - the entry-size limit refusing an oversized entry.
//!
//! ```text
//! cargo run --example notebook
//! ```

mod common;

use common::{Calc, Reply};
use diag_lang::Renderer;
use repl_lang::{Feed, Session};

/// One step of the script: a line to feed, or an interrupt.
enum Step {
    Line(&'static str),
    Interrupt,
}

const SCRIPT: &[Step] = &[
    Step::Line("let rate = 7  # percent"),
    Step::Line(""),
    Step::Line("let principal = 2_500"),
    Step::Line("let interest = principal * rate /"),
    Step::Line("    100"),
    Step::Line("interest + principal"),
    Step::Line("(principal -"),
    Step::Interrupt,
    Step::Line("let rate = 9"),
    Step::Line("interest / (rate - 7)"),
    Step::Line("2 ^ 64"),
    Step::Line("1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11 + 12 + 13 + 14 + 15 + 16"),
];

fn main() {
    // A deliberately small limit so the last line of the script trips it.
    let mut session = Session::with_limit(64);
    let mut calc = Calc::new();
    let renderer = Renderer::new();

    for step in SCRIPT {
        let line = match step {
            Step::Line(line) => *line,
            Step::Interrupt => {
                if session.cancel() {
                    println!("        ^C  (entry discarded)");
                }
                continue;
            }
        };

        let cell = session.len() + 1;
        if session.is_pending() {
            println!("   ...: {line}");
        } else {
            println!("In [{cell}]: {line}");
        }

        match session.feed(line, |input| calc.run(input)) {
            Ok(Feed::Complete { entry, value }) => {
                let n = entry.number();
                match value {
                    Ok(Reply::Value(v)) => println!("Out[{n}]: {v}"),
                    Ok(Reply::Bound { name, value }) => println!("Out[{n}]: {name} = {value}"),
                    Err(diagnostics) => {
                        for diagnostic in &diagnostics {
                            print!("{}", renderer.render(diagnostic, session.sources()));
                        }
                    }
                }
                println!();
            }
            Ok(Feed::Incomplete) => {}
            Ok(Feed::Empty) => println!(),
            Err(error) => println!("error: {error}\n"),
        }
    }

    println!("The session holds {} entries:", session.len());
    // Continuation lines line up under the text column: 2 + 9 + 1 spaces.
    let indent = format!("\n{}", " ".repeat(12));
    for (_, entry) in session.sources().iter() {
        let text = entry.text().trim_end().replace('\n', &indent);
        println!("  {:<9} {text}", entry.name());
    }
}
