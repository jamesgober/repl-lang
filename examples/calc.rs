//! A complete calculator REPL over standard input.
//!
//! Type expressions and `let` bindings. An entry that stops partway through —
//! an open parenthesis, a trailing operator, `let x =` — continues on the next
//! line under a `...` prompt. Errors render with source context, including
//! errors that point back into earlier entries.
//!
//! ```text
//! cargo run --example calc
//! calc[1]> let width = 12
//! width = 12
//! calc[2]> let area = width * (3 +
//!     ...>   4)
//! area = 84
//! calc[3]> let width = 1
//! error: `width` is already defined
//! ```
//!
//! Commands: `:history` lists the session's entries, `:quit` (or end of input)
//! exits. It also runs non-interactively: `echo "1 + 2" | cargo run --example calc`.

mod common;

use std::io::{self, BufRead, Write};

use common::{Calc, Reply};
use diag_lang::Renderer;
use repl_lang::{Feed, Session, SessionError};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut lines = stdin.lock();

    let mut session = Session::new();
    let mut calc = Calc::new();
    let renderer = Renderer::new();
    let mut line = String::new();

    loop {
        if session.is_pending() {
            write!(stdout, "    ...> ")?;
        } else {
            write!(stdout, "calc[{}]> ", session.len() + 1)?;
        }
        stdout.flush()?;

        line.clear();
        if lines.read_line(&mut line)? == 0 {
            if session.cancel() {
                writeln!(stdout, "\nerror: input ended inside an unfinished entry")?;
            }
            writeln!(stdout)?;
            return Ok(());
        }

        if !session.is_pending() {
            match line.trim() {
                ":quit" => return Ok(()),
                ":history" => {
                    for (_, entry) in session.sources().iter() {
                        write!(stdout, "{:>10} | {}", entry.name(), entry.text())?;
                    }
                    continue;
                }
                _ => {}
            }
        }

        match session.feed(&line, |input| calc.run(input)) {
            Ok(Feed::Complete {
                value: Ok(Reply::Value(n)),
                ..
            }) => writeln!(stdout, "{n}")?,
            Ok(Feed::Complete {
                value: Ok(Reply::Bound { name, value }),
                ..
            }) => {
                writeln!(stdout, "{name} = {value}")?;
            }
            Ok(Feed::Complete {
                value: Err(diagnostics),
                ..
            }) => {
                for diagnostic in &diagnostics {
                    write!(stdout, "{}", renderer.render(diagnostic, session.sources()))?;
                }
            }
            Ok(Feed::Incomplete | Feed::Empty) => {}
            Err(error @ SessionError::TooLong { .. }) => writeln!(stdout, "error: {error}")?,
            Err(error) => {
                writeln!(stdout, "error: {error}")?;
                return Ok(());
            }
        }
    }
}
