//! The line editor, driven by a scripted sequence of keystrokes.
//!
//! A real host reads key events from a terminal library and maps them to
//! `Edit` commands; this example replays a fixed sequence instead, so it runs
//! anywhere and prints the same output every time. After each step it draws
//! the line the way a terminal would, with a caret under the cursor column —
//! note how the wide characters in `"界"` take two columns, and how the
//! combining accent in `"é"` never separates from its letter.
//!
//! ```text
//! cargo run --example line_editing
//! ```

use repl_lang::{Edit, Editor};
use unicode_lang::str_width;

/// One keystroke: a label for the transcript and what it does.
enum Key {
    Type(&'static str),
    Edit(&'static str, Edit),
    Enter,
}

const PROMPT: &str = "> ";

fn main() {
    let mut editor = Editor::with_history(100);
    // History restored from an earlier session.
    for saved in ["let width = 12", "width * 2"] {
        editor.add_history(saved);
    }

    let keys = [
        Key::Type("print hello"),
        Key::Edit("Ctrl-W  (kill word)", Edit::KillWordLeft),
        Key::Type("世界"),
        Key::Edit("Home", Edit::Home),
        Key::Edit("Alt-F   (word right)", Edit::WordRight),
        Key::Type("ln"),
        Key::Edit("End", Edit::End),
        Key::Edit("Backspace", Edit::Backspace),
        Key::Edit("Ctrl-Y  (yank)", Edit::Yank),
        Key::Enter,
        Key::Type("caf\u{0065}\u{0301}"),
        Key::Edit("←", Edit::Left),
        Key::Edit("←", Edit::Left),
        Key::Edit("Ctrl-K  (kill to end)", Edit::KillToEnd),
        Key::Edit("Ctrl-U  (kill to start)", Edit::KillToStart),
        Key::Edit("↑", Edit::HistoryPrev),
        Key::Edit("↑", Edit::HistoryPrev),
        Key::Edit("↑", Edit::HistoryPrev),
        Key::Edit("↓", Edit::HistoryNext),
        Key::Type(" + 1"),
        Key::Enter,
    ];

    for key in &keys {
        let (label, changed) = match key {
            Key::Type(text) => (format!("type {text:?}"), editor.insert(text)),
            Key::Edit(label, edit) => ((*label).to_owned(), editor.apply(*edit)),
            Key::Enter => {
                let line = editor.submit().to_owned();
                println!("{} submitted {line:?}\n", pad("Enter"));
                continue;
            }
        };
        let note = if changed { "" } else { "  (no change)" };
        println!("{} {PROMPT}{}{note}", pad(&label), editor.line());
        println!(
            "{} {}^",
            pad(""),
            " ".repeat(PROMPT.len() + editor.column())
        );
    }

    println!("History, oldest first:");
    for (i, line) in editor.history().enumerate() {
        println!("  {:>2}  {line}", i + 1);
    }
}

/// Pads `label` to a fixed number of terminal columns. `format!("{:<26}")`
/// pads by `char` count, which misaligns labels containing wide characters.
fn pad(label: &str) -> String {
    let fill = 26_usize.saturating_sub(str_width(label));
    format!("{label}{}", " ".repeat(fill))
}
