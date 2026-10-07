//! End-to-end editing scenarios through the public `Editor` API.

use repl_lang::{Edit, Editor, Feed, Session, Status};

/// Applies each edit, returning whether each one changed anything.
fn press(editor: &mut Editor, edits: &[Edit]) -> Vec<bool> {
    edits.iter().map(|&edit| editor.apply(edit)).collect()
}

#[test]
fn test_typing_character_by_character_matches_insert() {
    let text = "let π = 3.14159 # 円周率";
    let mut by_char = Editor::new();
    for c in text.chars() {
        assert!(by_char.apply(Edit::Insert(c)));
    }
    let mut by_str = Editor::new();
    assert!(by_str.insert(text));
    assert_eq!(by_char.line(), by_str.line());
    assert_eq!(by_char.cursor(), by_str.cursor());
    assert_eq!(by_char.column(), by_str.column());
}

#[test]
fn test_walking_left_then_right_visits_same_positions() {
    let mut editor = Editor::new();
    editor.insert("a世e\u{0301}😀z");
    let mut left = vec![editor.cursor()];
    while editor.apply(Edit::Left) {
        left.push(editor.cursor());
    }
    let mut right = vec![editor.cursor()];
    while editor.apply(Edit::Right) {
        right.push(editor.cursor());
    }
    right.reverse();
    assert_eq!(left, right);
    // a, 世, e+◌́, 😀, z: five units, six positions.
    assert_eq!(left.len(), 6);
}

#[test]
fn test_word_hops_across_a_typical_expression() {
    let mut editor = Editor::new();
    editor.insert("total_cost = price * (1 + tax_rate)");
    let mut stops = Vec::new();
    while editor.apply(Edit::WordLeft) {
        stops.push(editor.line()[editor.cursor()..].to_owned());
    }
    assert_eq!(
        stops,
        [
            "tax_rate)",
            "1 + tax_rate)",
            "price * (1 + tax_rate)",
            "total_cost = price * (1 + tax_rate)"
        ]
    );
}

#[test]
fn test_kill_and_yank_move_text_around() {
    let mut editor = Editor::new();
    editor.insert("second first");
    assert_eq!(
        press(&mut editor, &[Edit::KillWordLeft, Edit::Home, Edit::Yank]),
        [true, true, true]
    );
    editor.insert(" ");
    assert_eq!(editor.line(), "first second ");
    assert!(editor.apply(Edit::End));
    assert!(editor.apply(Edit::Backspace));
    assert_eq!(editor.line(), "first second");
}

#[test]
fn test_history_round_trips_through_persistence() {
    let mut first = Editor::new();
    for line in ["let a = 1", "a + 1", "a * 10"] {
        first.insert(line);
        first.submit();
    }
    let saved: String = first.history().flat_map(|line| [line, "\n"]).collect();

    let mut second = Editor::new();
    for line in saved.lines() {
        second.add_history(line);
    }
    assert!(first.history().eq(second.history()));
    assert!(second.apply(Edit::HistoryPrev));
    assert_eq!(second.line(), "a * 10");
}

#[test]
fn test_browsing_does_not_disturb_the_draft_until_submit() {
    let mut editor = Editor::new();
    editor.add_history("one");
    editor.add_history("two");
    editor.insert("thr");
    assert_eq!(
        press(
            &mut editor,
            &[
                Edit::HistoryPrev,
                Edit::HistoryPrev,
                Edit::HistoryNext,
                Edit::HistoryNext
            ]
        ),
        [true, true, true, true]
    );
    assert_eq!(editor.line(), "thr");
    editor.insert("ee");
    assert_eq!(editor.submit(), "three");
    assert!(editor.history().eq(["one", "two", "three"]));
}

#[test]
fn test_editor_feeds_a_session() {
    let mut editor = Editor::new();
    let mut session = Session::new();
    let mut results = Vec::new();
    for line in ["[1, 2,", "3]"] {
        editor.insert(line);
        let submitted = editor.submit();
        let feed = session
            .feed(submitted, |input| {
                if input.text().matches('[').count() > input.text().matches(']').count() {
                    Status::Incomplete
                } else {
                    Status::Complete(input.text().len())
                }
            })
            .unwrap_or_else(|e| panic!("{e}"));
        results.push(feed);
    }
    assert_eq!(results[0], Feed::Incomplete);
    assert!(matches!(results[1], Feed::Complete { value: 10, .. }));
    // Each physical line was recorded separately in the editor history.
    assert!(editor.history().eq(["[1, 2,", "3]"]));
}

#[test]
fn test_long_line_editing_stays_consistent() {
    let mut editor = Editor::with_history(4);
    let word = "αβγ_δ ";
    for _ in 0..2_000 {
        editor.insert(word);
    }
    assert_eq!(editor.column(), 2_000 * 6);
    assert!(editor.apply(Edit::Home));
    for _ in 0..1_000 {
        assert!(editor.apply(Edit::KillWordRight));
    }
    // The first kill takes a bare word; each later one a space and a word.
    assert_eq!(editor.line(), format!(" {}", word.repeat(1_000)));
    assert_eq!(editor.cursor(), 0);
}
