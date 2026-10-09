//! Property tests: the editor and the session checked against simple reference
//! models over arbitrary inputs.
//!
//! The editor model works on a `Vec<char>` with a cursor counted in characters,
//! so it shares none of the implementation's byte-offset arithmetic or string
//! surgery. The session model is a plain string accumulator. After every step
//! the real type and its model must agree exactly.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::VecDeque;

use diag_lang::{Diagnostic, Label, Severity};
use proptest::prelude::*;
use repl_lang::{Edit, Editor, Feed, Session, SessionError, Span, Status};
use unicode_lang::{char_width, is_xid_continue, str_width};

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

/// Characters chosen to stress the unit and word rules: ASCII word and
/// separator characters, a precomposed accent, a combining accent, a joiner, a
/// wide ideograph, an astral emoji, and control characters: refused when typed
/// or pasted, while a history entry keeps tabs and newlines (and turns `\r`
/// into a newline).
fn any_char() -> impl Strategy<Value = char> {
    prop::sample::select(vec![
        'a', 'b', 'Z', '_', '7', ' ', '.', '(', 'é', '\u{0301}', '\u{200D}', '世', '😀', '\t',
        '\n', '\r', '\u{1b}',
    ])
}

/// Columns a character takes in the model: a tab counts one, as the editor
/// documents; everything else follows the Unicode tables.
fn model_width(c: char) -> usize {
    if c == '\t' { 1 } else { char_width(c) }
}

/// Whether `c` begins a display unit in the model: anything with width, and a
/// newline.
fn starts_unit(c: char) -> bool {
    model_width(c) != 0 || c == '\n'
}

/// What a history entry keeps of `line`, written as plain string replacement.
fn recallable(line: &str) -> String {
    line.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect()
}

#[derive(Clone, Debug)]
enum Op {
    Apply(Edit),
    Insert(String),
    Submit,
    Clear,
    AddHistory(String),
}

fn any_edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        any_char().prop_map(Edit::Insert),
        prop::sample::select(vec![
            Edit::Backspace,
            Edit::Delete,
            Edit::Left,
            Edit::Right,
            Edit::WordLeft,
            Edit::WordRight,
            Edit::Home,
            Edit::End,
            Edit::KillToStart,
            Edit::KillToEnd,
            Edit::KillWordLeft,
            Edit::KillWordRight,
            Edit::Yank,
            Edit::HistoryPrev,
            Edit::HistoryNext,
        ]),
    ]
}

fn any_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any_char(), 0..6).prop_map(|chars| chars.into_iter().collect())
}

fn any_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => any_edit().prop_map(Op::Apply),
        2 => any_text().prop_map(Op::Insert),
        1 => Just(Op::Submit),
        1 => Just(Op::Clear),
        1 => any_text().prop_map(Op::AddHistory),
    ]
}

/// The reference editor.
struct Model {
    chars: Vec<char>,
    cursor: usize,
    kill: Vec<char>,
    history: VecDeque<String>,
    capacity: usize,
    browsing: Option<usize>,
    draft: Vec<char>,
}

impl Model {
    fn new(capacity: usize) -> Self {
        Self {
            chars: Vec::new(),
            cursor: 0,
            kill: Vec::new(),
            history: VecDeque::new(),
            capacity,
            browsing: None,
            draft: Vec::new(),
        }
    }

    fn line(&self) -> String {
        self.chars.iter().collect()
    }

    fn byte_cursor(&self) -> usize {
        self.chars[..self.cursor].iter().map(|c| c.len_utf8()).sum()
    }

    fn next(&self, mut i: usize) -> usize {
        if i >= self.chars.len() {
            return i;
        }
        i += 1;
        while i < self.chars.len() && !starts_unit(self.chars[i]) {
            i += 1;
        }
        i
    }

    fn prev(&self, mut i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        i -= 1;
        while i > 0 && !starts_unit(self.chars[i]) {
            i -= 1;
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        while i < self.chars.len() && !is_xid_continue(self.chars[i]) {
            i = self.next(i);
        }
        while i < self.chars.len() && is_xid_continue(self.chars[i]) {
            i = self.next(i);
        }
        i
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && !is_xid_continue(self.chars[self.prev(i)]) {
            i = self.prev(i);
        }
        while i > 0 && is_xid_continue(self.chars[self.prev(i)]) {
            i = self.prev(i);
        }
        i
    }

    fn moved(&mut self, to: usize) -> bool {
        let changed = to != self.cursor;
        self.cursor = to;
        changed
    }

    fn kill(&mut self, start: usize, end: usize) -> bool {
        if start == end {
            return false;
        }
        self.kill = self.chars.drain(start..end).collect();
        self.cursor = start;
        true
    }

    fn insert(&mut self, text: &[char]) -> bool {
        let mut any = false;
        for &c in text.iter().filter(|c| !c.is_control()) {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
            any = true;
        }
        any
    }

    fn record(&mut self, line: &str) {
        if self.capacity == 0
            || line.trim().is_empty()
            || self.history.back().is_some_and(|newest| newest == line)
        {
            return;
        }
        if self.history.len() == self.capacity {
            self.history.pop_front();
            self.browsing = self.browsing.map(|i| i.saturating_sub(1));
        }
        self.history.push_back(line.to_owned());
    }

    fn load(&mut self, index: usize) {
        self.chars = self.history[index].chars().collect();
        self.cursor = self.chars.len();
        self.browsing = Some(index);
    }

    fn apply(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Insert(c) => self.insert(&[c]),
            Edit::Backspace => {
                if self.cursor == 0 {
                    return false;
                }
                let start = self.prev(self.cursor);
                self.chars.drain(start..self.cursor);
                self.cursor = start;
                true
            }
            Edit::Delete => {
                let end = self.next(self.cursor);
                if end == self.cursor {
                    return false;
                }
                self.chars.drain(self.cursor..end);
                true
            }
            Edit::Left => self.moved(self.prev(self.cursor)),
            Edit::Right => self.moved(self.next(self.cursor)),
            Edit::WordLeft => self.moved(self.word_left(self.cursor)),
            Edit::WordRight => self.moved(self.word_right(self.cursor)),
            Edit::Home => self.moved(0),
            Edit::End => self.moved(self.chars.len()),
            Edit::KillToStart => self.kill(0, self.cursor),
            Edit::KillToEnd => self.kill(self.cursor, self.chars.len()),
            Edit::KillWordLeft => self.kill(self.word_left(self.cursor), self.cursor),
            Edit::KillWordRight => self.kill(self.cursor, self.word_right(self.cursor)),
            Edit::Yank => {
                // The kill buffer goes back verbatim, newlines and tabs from
                // a recalled entry included.
                if self.kill.is_empty() {
                    return false;
                }
                let kill = self.kill.clone();
                let n = kill.len();
                self.chars.splice(self.cursor..self.cursor, kill);
                self.cursor += n;
                true
            }
            Edit::HistoryPrev => match self.browsing {
                None if self.history.is_empty() => false,
                None => {
                    self.draft = std::mem::take(&mut self.chars);
                    self.load(self.history.len() - 1);
                    true
                }
                Some(0) => false,
                Some(i) => {
                    self.load(i - 1);
                    true
                }
            },
            Edit::HistoryNext => match self.browsing {
                None => false,
                Some(i) if i + 1 < self.history.len() => {
                    self.load(i + 1);
                    true
                }
                Some(_) => {
                    self.chars = std::mem::take(&mut self.draft);
                    self.cursor = self.chars.len();
                    self.browsing = None;
                    true
                }
            },
            other => panic!("model does not know {other:?}"),
        }
    }

    fn submit(&mut self) -> String {
        let line = self.line();
        self.chars.clear();
        self.cursor = 0;
        self.browsing = None;
        self.draft.clear();
        self.record(&line);
        line
    }

    fn clear(&mut self) -> bool {
        self.browsing = None;
        self.draft.clear();
        let changed = !self.chars.is_empty();
        self.chars.clear();
        self.cursor = 0;
        changed
    }
}

fn assert_same(editor: &Editor, model: &Model, step: &Op) {
    assert_eq!(editor.line(), model.line(), "line after {step:?}");
    assert_eq!(
        editor.cursor(),
        model.byte_cursor(),
        "cursor after {step:?}"
    );
    assert!(editor.line().is_char_boundary(editor.cursor()));
    let before = &editor.line()[..editor.cursor()];
    let own_line = before.rsplit('\n').next().unwrap_or("");
    assert_eq!(
        editor.column(),
        own_line.chars().map(model_width).sum::<usize>(),
        "column after {step:?}"
    );
    if !before.contains(['\n', '\t']) {
        assert_eq!(editor.column(), str_width(before));
    }
    assert!(
        editor
            .history()
            .eq(model.history.iter().map(String::as_str)),
        "history after {step:?}"
    );
}

/// Runs `ops` on an editor and on the model, both with history `capacity`,
/// and checks that they agree after every step.
fn check_editor(capacity: usize, ops: &[Op]) -> Result<(), TestCaseError> {
    let mut editor = Editor::with_history(capacity);
    let mut model = Model::new(capacity);
    for op in ops {
        match op {
            Op::Apply(edit) => {
                prop_assert_eq!(
                    editor.apply(*edit),
                    model.apply(*edit),
                    "result of {:?}",
                    op
                );
            }
            Op::Insert(text) => {
                let chars: Vec<char> = text.chars().collect();
                prop_assert_eq!(editor.insert(text), model.insert(&chars));
            }
            Op::Submit => prop_assert_eq!(editor.submit(), model.submit()),
            Op::Clear => prop_assert_eq!(editor.clear(), model.clear()),
            Op::AddHistory(line) => {
                editor.add_history(line);
                model.record(&recallable(line));
            }
        }
        assert_same(&editor, &model, op);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every operation, in any order, does exactly what the reference does.
    #[test]
    fn prop_editor_matches_model(
        capacity in 0_usize..4,
        ops in prop::collection::vec(any_op(), 0..80),
    ) {
        check_editor(capacity, &ops)?;
    }

    /// The same, starting from a recalled multi-line history entry, so the
    /// line holds newlines and tabs for the operations that follow.
    #[test]
    fn prop_editor_matches_model_on_recalled_multi_line_entries(
        entry in prop::collection::vec(
            prop::sample::select(vec!['a', 'b', ' ', '(', 'é', '\u{0301}', '世', '\t', '\n', '\r']),
            1..12,
        ),
        ops in prop::collection::vec(any_op(), 0..40),
    ) {
        let entry: String = entry.into_iter().collect();
        let mut all = vec![Op::AddHistory(format!("x{entry}")), Op::Apply(Edit::HistoryPrev)];
        all.extend(ops);
        check_editor(3, &all)?;
    }

    /// Typing a string and deleting it unit by unit leaves an empty line, and
    /// the number of units equals the number of width-bearing characters.
    #[test]
    fn prop_backspace_undoes_typing(text in any_text()) {
        let mut editor = Editor::new();
        editor.insert(&text);
        let typed: String = text.chars().filter(|c| !c.is_control()).collect();
        prop_assert_eq!(editor.line(), typed.as_str());
        let mut units = 0;
        while editor.apply(Edit::Backspace) {
            units += 1;
        }
        prop_assert_eq!(editor.line(), "");
        let starts = typed.chars().enumerate().filter(|&(i, c)| i == 0 || char_width(c) != 0).count();
        prop_assert_eq!(units, starts);
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Lines over a small alphabet with brackets, blanks, and both terminators.
fn any_line() -> impl Strategy<Value = String> {
    let body = prop::collection::vec(prop::sample::select(vec!['a', ' ', '(', ')', '\t']), 0..8)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    (body, prop::sample::select(vec!["", "\n", "\r\n"])).prop_map(|(b, end)| b + end)
}

/// Incomplete while `(` outnumber `)`; complete otherwise, echoing the text.
fn brackets(input: repl_lang::Input<'_>) -> Status<(String, u32, u32)> {
    let open = input.text().matches('(').count();
    let close = input.text().matches(')').count();
    if open > close {
        Status::Incomplete
    } else {
        Status::Complete((
            input.text().to_owned(),
            input.base().to_u32(),
            input.number(),
        ))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The session agrees with a plain accumulator on every feed: what is
    /// pending, what is committed, at which base, under which number, and when
    /// the limit refuses a line.
    #[test]
    fn prop_session_matches_model(
        limit in prop_oneof![Just(usize::MAX), 0_usize..40],
        lines in prop::collection::vec(any_line(), 0..40),
    ) {
        let mut session = Session::with_limit(limit);
        let mut pending = String::new();
        let mut committed: Vec<String> = Vec::new();
        let mut base = 0_u32;

        for line in &lines {
            let result = session.feed(line, brackets);

            let stripped = line.strip_suffix('\n').map_or(line.as_str(), |l| l.strip_suffix('\r').unwrap_or(l));
            if pending.is_empty() && stripped.trim().is_empty() {
                prop_assert_eq!(result, Ok(Feed::Empty));
                continue;
            }
            let len = pending.len() + stripped.len() + 1;
            if len > limit {
                prop_assert_eq!(result, Err(SessionError::TooLong { len, limit }));
                pending.clear();
                continue;
            }
            pending.push_str(stripped);
            pending.push('\n');
            if pending.matches('(').count() > pending.matches(')').count() {
                prop_assert_eq!(result, Ok(Feed::Incomplete));
                continue;
            }
            let number = u32::try_from(committed.len() + 1).unwrap();
            let span = Span::new(base, base + u32::try_from(pending.len()).unwrap());
            match result {
                Ok(Feed::Complete { entry, value: (text, seen_base, seen_number) }) => {
                    prop_assert_eq!(&text, &pending);
                    prop_assert_eq!(seen_base, base);
                    prop_assert_eq!(seen_number, number);
                    prop_assert_eq!(entry.number(), number);
                    prop_assert_eq!(entry.span(), span);
                }
                other => prop_assert!(false, "expected Complete, got {:?}", other),
            }
            base = span.end().to_u32();
            committed.push(std::mem::take(&mut pending));
        }

        prop_assert_eq!(session.pending(), pending.as_str());
        prop_assert_eq!(session.len(), committed.len());
        for (i, text) in committed.iter().enumerate() {
            let number = u32::try_from(i + 1).unwrap();
            let entry = session.entry(number).unwrap();
            prop_assert_eq!(entry.text(), text.as_str());
            prop_assert_eq!(entry.name(), format!("<repl:{number}>"));
        }
    }

    /// `is_incomplete` holds exactly when there is an error and every error
    /// starts at or after the end of the non-whitespace text.
    #[test]
    fn prop_is_incomplete_matches_definition(
        body in "[a-z (]{0,6}[ \t]{0,3}",
        errors in prop::collection::vec((0_u32..12, 0_usize..4), 0..4),
    ) {
        // Two earlier entries, so the input under test does not start at 0.
        let mut session = Session::new();
        session.feed("pad
", |_| Status::Complete(())).unwrap(); // 0..4
        session.feed("x", |_| Status::Complete(())).unwrap(); // 4..6
        let base = 6_u32;

        let severities = [Severity::Error, Severity::Warning, Severity::Note, Severity::Help];
        let diagnostics: Vec<Diagnostic> = errors
            .iter()
            .map(|&(at, sev)| {
                Diagnostic::new(severities[sev], "m", Label::new(Span::empty(base + at), "l"))
            })
            .collect();

        let end = base + u32::try_from(body.trim_end().len()).unwrap();
        let errs: Vec<u32> = errors.iter().filter(|e| e.1 == 0).map(|e| base + e.0).collect();
        let expected = !errs.is_empty() && errs.iter().all(|&at| at >= end);

        session.feed(&body, |input| {
            assert_eq!(input.base().to_u32(), base);
            assert_eq!(input.is_incomplete(&diagnostics), expected, "{body:?} {errors:?}");
            Status::Complete(())
        }).unwrap();
    }

    /// A parser that reports spans relative to the entry text gets, through
    /// `is_incomplete_relative` and in any entry, exactly the verdict of the
    /// definition applied to the text alone.
    #[test]
    fn prop_is_incomplete_relative_matches_definition(
        pad in "[a-z]{0,24}",
        body in "[a-z (]{0,10}[ \t]{0,3}",
        errors in prop::collection::vec((0_u32..16, 0_u32..4, 0_usize..4), 0..4),
    ) {
        let mut session = Session::new();
        session.feed(&pad, |_| Status::Complete(())).unwrap();
        let end = u32::try_from(body.trim_end().len()).unwrap();
        let severities = [Severity::Error, Severity::Warning, Severity::Note, Severity::Help];
        let diagnostics: Vec<Diagnostic> = errors
            .iter()
            .map(|&(s, w, sev)| Diagnostic::new(severities[sev], "m", Label::new(Span::new(s, s + w), "l")))
            .collect();
        let errs: Vec<u32> = errors.iter().filter(|e| e.2 == 0).map(|e| e.0).collect();
        let expected = !errs.is_empty() && errs.iter().all(|&at| at >= end);

        session.feed(&body, |input| {
            assert_eq!(input.is_incomplete_relative(&diagnostics), expected, "{pad:?} {body:?} {errors:?}");
            Status::Complete(())
        }).unwrap();
    }

    /// The two methods are one rule in two coordinate spaces: shifting every
    /// span by the entry's base turns one verdict into the other.
    #[test]
    fn prop_is_incomplete_global_equals_relative_shifted_by_base(
        pad in "[a-z]{0,24}",
        body in "[a-z (]{0,10}[ \t]{0,3}",
        errors in prop::collection::vec((0_u32..16, 0_u32..4), 0..4),
    ) {
        let mut session = Session::new();
        session.feed(&pad, |_| Status::Complete(())).unwrap();
        session.feed(&body, |input| {
            let base = input.base().to_u32();
            let make = |shift: u32| -> Vec<Diagnostic> {
                errors
                    .iter()
                    .map(|&(s, w)| {
                        Diagnostic::new(Severity::Error, "m", Label::new(Span::new(s + shift, s + w + shift), "l"))
                    })
                    .collect()
            };
            assert_eq!(input.is_incomplete(&make(base)), input.is_incomplete_relative(&make(0)));
            Status::Complete(())
        }).unwrap();
    }
}
