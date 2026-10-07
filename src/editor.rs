//! A terminal-agnostic single-line editor with history and a kill buffer.

use alloc::{collections::VecDeque, string::String};

use crate::{
    Edit,
    text::{next_unit, prev_unit, text_width, word_left, word_right},
};

/// The editing state of one input line: its text, the cursor, a kill buffer,
/// and a bounded history of submitted lines.
///
/// `Editor` is the "line editing" part of a REPL with the terminal taken out.
/// It holds no file descriptors and draws nothing. The host translates key
/// events into [`Edit`] commands, calls [`apply`](Editor::apply), and redraws
/// from [`line`](Editor::line) and [`column`](Editor::column) when `apply`
/// reports a change. On Enter it calls [`submit`](Editor::submit), which hands
/// back the finished line and records it in the history. That split makes the
/// editor identical on every platform and testable without a terminal.
///
/// Positions are byte offsets into [`line`](Editor::line) and always fall on a
/// `char` boundary, so slicing the line at [`cursor`](Editor::cursor) is
/// always valid. The cursor moves in display units (a visible character plus
/// any combining marks attached to it), so it never separates a letter from its
/// accent.
///
/// # Memory
///
/// The line, the kill buffer, and every history slot are reused rather than
/// reallocated. Once the history has filled to capacity, submitting a line
/// recycles the oldest slot's buffer, so a long session settles into editing
/// and submitting without touching the allocator except to grow a buffer past
/// its previous high-water mark.
///
/// # Examples
///
/// ```
/// use repl_lang::{Edit, Editor};
///
/// let mut editor = Editor::new();
/// editor.insert("let x = 1");
/// assert_eq!(editor.submit(), "let x = 1");
/// assert_eq!(editor.line(), "");
///
/// // The submitted line is one step back in the history.
/// assert!(editor.apply(Edit::HistoryPrev));
/// assert_eq!(editor.line(), "let x = 1");
/// ```
#[derive(Clone, Debug)]
pub struct Editor {
    /// The line being edited.
    line: String,
    /// Byte offset of the cursor in `line`; always on a `char` boundary.
    cursor: usize,
    /// Text removed by the last kill command, for `Yank`.
    kill: String,
    /// The last line returned by `submit`; its buffer becomes the next line's.
    submitted: String,
    /// Submitted lines, oldest first.
    history: VecDeque<String>,
    /// Maximum number of history entries kept.
    capacity: usize,
    /// Index into `history` while browsing it with `HistoryPrev`/`HistoryNext`.
    browsing: Option<usize>,
    /// The line being typed when browsing started, restored when browsing ends.
    draft: String,
}

impl Default for Editor {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    /// The number of history entries [`Editor::new`] keeps.
    pub const DEFAULT_HISTORY: usize = 1000;

    /// Creates an empty editor that keeps the last
    /// [`DEFAULT_HISTORY`](Editor::DEFAULT_HISTORY) submitted lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let editor = Editor::new();
    /// assert_eq!(editor.line(), "");
    /// assert_eq!(editor.cursor(), 0);
    /// assert_eq!(editor.history().len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_history(Self::DEFAULT_HISTORY)
    }

    /// Creates an empty editor that keeps at most `capacity` history entries.
    ///
    /// When the history is full, submitting a line drops the oldest entry. A
    /// capacity of `0` disables history: nothing is recorded and the history
    /// commands do nothing. No history memory is reserved up front; slots are
    /// allocated as lines are submitted, then recycled once the limit is
    /// reached.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::with_history(2);
    /// for line in ["a", "b", "c"] {
    ///     editor.insert(line);
    ///     editor.submit();
    /// }
    /// // Only the two most recent lines are kept, oldest first.
    /// assert!(editor.history().eq(["b", "c"]));
    /// ```
    #[must_use]
    pub fn with_history(capacity: usize) -> Self {
        Self {
            line: String::new(),
            cursor: 0,
            kill: String::new(),
            submitted: String::new(),
            history: VecDeque::new(),
            capacity,
            browsing: None,
            draft: String::new(),
        }
    }

    /// The text of the line being edited.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("1 + 2");
    /// assert_eq!(editor.line(), "1 + 2");
    /// ```
    #[inline]
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The cursor position as a byte offset into [`line`](Editor::line).
    ///
    /// The offset is always on a `char` boundary, so `&line[..cursor]` and
    /// `&line[cursor..]` are always valid. Use it to find the word under the
    /// cursor for completion; use [`column`](Editor::column) to place the
    /// terminal cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Edit, Editor};
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("pri");
    /// assert_eq!(editor.cursor(), 3);
    ///
    /// // The text before the cursor is the prefix to complete.
    /// let prefix = &editor.line()[..editor.cursor()];
    /// assert_eq!(prefix, "pri");
    ///
    /// assert!(editor.apply(Edit::Home));
    /// assert_eq!(editor.cursor(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The cursor's position in terminal columns, counted from the start of the
    /// line.
    ///
    /// This is the display width of the text before the cursor: wide characters
    /// (CJK ideographs, most emoji) count two columns and combining marks count
    /// none. After drawing the prompt and the line, a host moves the terminal
    /// cursor to `prompt_width + column()`.
    ///
    /// The width is computed on each call, in time linear in the text before
    /// the cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("x = 世界");
    /// assert_eq!(editor.cursor(), 10); // bytes: 4 ASCII + 2 × 3
    /// assert_eq!(editor.column(), 8);  // columns: 4 ASCII + 2 × 2
    /// ```
    #[must_use]
    pub fn column(&self) -> usize {
        text_width(&self.line[..self.cursor])
    }

    /// Applies one editing command and reports whether the line or the cursor
    /// changed.
    ///
    /// A command that cannot do anything — moving left at the start of the
    /// line, deleting at the end, yanking an empty kill buffer, inserting a
    /// control character — leaves the editor untouched and returns `false`, so
    /// the host can skip a redraw or ring the bell.
    ///
    /// The kill commands replace the kill buffer with the text they remove;
    /// a kill that removes nothing leaves the buffer as it was.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Edit, Editor};
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("hello world");
    ///
    /// assert!(editor.apply(Edit::KillWordLeft));
    /// assert_eq!(editor.line(), "hello ");
    ///
    /// assert!(editor.apply(Edit::Home));
    /// assert!(editor.apply(Edit::Yank));
    /// assert_eq!(editor.line(), "worldhello ");
    /// assert_eq!(editor.cursor(), 5); // just past the yanked text
    ///
    /// // Commands with nothing to do report `false`.
    /// assert!(editor.apply(Edit::Home));
    /// assert!(!editor.apply(Edit::Left));
    /// assert!(!editor.apply(Edit::Insert('\t'))); // control characters are refused
    /// ```
    pub fn apply(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Insert(c) => self.insert_char(c),
            Edit::Backspace => self.backspace(),
            Edit::Delete => self.delete(),
            Edit::Left => self.move_to(prev_unit(&self.line, self.cursor)),
            Edit::Right => self.move_to(next_unit(&self.line, self.cursor)),
            Edit::WordLeft => self.move_to(word_left(&self.line, self.cursor)),
            Edit::WordRight => self.move_to(word_right(&self.line, self.cursor)),
            Edit::Home => self.move_to(0),
            Edit::End => self.move_to(self.line.len()),
            Edit::KillToStart => self.kill_range(0, self.cursor),
            Edit::KillToEnd => self.kill_range(self.cursor, self.line.len()),
            Edit::KillWordLeft => self.kill_range(word_left(&self.line, self.cursor), self.cursor),
            Edit::KillWordRight => {
                self.kill_range(self.cursor, word_right(&self.line, self.cursor))
            }
            Edit::Yank => self.yank(),
            Edit::HistoryPrev => self.history_prev(),
            Edit::HistoryNext => self.history_next(),
        }
    }

    /// Inserts `text` at the cursor, as a paste would, and reports whether
    /// anything was inserted.
    ///
    /// Control characters in `text` are dropped, exactly as
    /// [`Edit::Insert`] refuses them one at a time; everything else is
    /// inserted in a single pass. A host that receives a multi-line paste
    /// should split it into lines and submit them one by one.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Edit, Editor};
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("fn main() {}");
    /// assert!(editor.apply(Edit::Left));
    /// editor.insert(" body ");
    /// assert_eq!(editor.line(), "fn main() { body }");
    ///
    /// // Control characters are dropped from pasted text.
    /// let mut editor = Editor::new();
    /// editor.insert("a\tb\u{1b}c");
    /// assert_eq!(editor.line(), "abc");
    /// assert!(!editor.insert("\n"));
    /// ```
    pub fn insert(&mut self, text: &str) -> bool {
        let before = self.line.len();
        for segment in text.split(char::is_control) {
            self.line.insert_str(self.cursor, segment);
            self.cursor += segment.len();
        }
        self.line.len() != before
    }

    /// Empties the line and moves the cursor to the start, keeping the kill
    /// buffer and the history. Returns `true` if the line was not already
    /// empty.
    ///
    /// This is the editor half of an interrupt (`Ctrl-C`): abandon what is
    /// being typed and start over. If the history was being browsed, browsing
    /// ends and the line parked when it began is dropped as well.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("half-typed");
    /// assert!(editor.clear());
    /// assert_eq!(editor.line(), "");
    /// assert!(!editor.clear());
    /// ```
    pub fn clear(&mut self) -> bool {
        self.forget_draft();
        let changed = !self.line.is_empty();
        self.line.clear();
        self.cursor = 0;
        changed
    }

    /// Finishes the line: returns its text, records it in the history, and
    /// leaves the editor empty for the next line.
    ///
    /// A line is recorded unless it is blank (empty or only whitespace) or
    /// identical to the most recent history entry, and only if the history
    /// capacity is not `0`.
    ///
    /// Submitting while browsing the history submits the recalled line as
    /// edited; the line parked when browsing began is dropped. Either way the
    /// next [`HistoryPrev`](Edit::HistoryPrev) starts from the newest entry.
    ///
    /// The returned text stays borrowed from the editor, which keeps the
    /// buffer for reuse: the next submit recycles it as the following line.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::new();
    /// editor.insert("1 + 1");
    /// assert_eq!(editor.submit(), "1 + 1");
    ///
    /// // Blank lines and immediate repeats are not recorded.
    /// editor.insert("   ");
    /// assert_eq!(editor.submit(), "   ");
    /// editor.insert("1 + 1");
    /// editor.submit();
    /// assert_eq!(editor.history().len(), 1);
    /// ```
    pub fn submit(&mut self) -> &str {
        self.forget_draft();
        core::mem::swap(&mut self.line, &mut self.submitted);
        self.line.clear();
        self.cursor = 0;
        record(
            &mut self.history,
            self.capacity,
            &self.submitted,
            &mut self.browsing,
        );
        &self.submitted
    }

    /// Records `line` in the history as if it had been submitted, without
    /// touching the line being edited.
    ///
    /// Use it to restore a history saved from an earlier session. The same
    /// rules as [`submit`](Editor::submit) apply: blank lines and repeats of
    /// the newest entry are skipped, and the oldest entry is dropped once the
    /// history is full. Control characters are removed first, exactly as
    /// [`insert`](Editor::insert) removes them, so a recalled entry can never
    /// put into the line something that could not have been typed there.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Edit, Editor};
    ///
    /// let saved = "let x = 1\nx * 2\n";
    /// let mut editor = Editor::new();
    /// for line in saved.lines() {
    ///     editor.add_history(line);
    /// }
    /// assert!(editor.apply(Edit::HistoryPrev));
    /// assert_eq!(editor.line(), "x * 2");
    /// ```
    pub fn add_history(&mut self, line: &str) {
        if line.contains(char::is_control) {
            let clean: String = line.chars().filter(|c| !c.is_control()).collect();
            record(&mut self.history, self.capacity, &clean, &mut self.browsing);
        } else {
            record(&mut self.history, self.capacity, line, &mut self.browsing);
        }
    }

    /// The history, oldest entry first.
    ///
    /// Iterate it in reverse for newest-first, or write it out line by line to
    /// persist the history between sessions.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Editor;
    ///
    /// let mut editor = Editor::new();
    /// for line in ["first", "second"] {
    ///     editor.insert(line);
    ///     editor.submit();
    /// }
    /// assert_eq!(editor.history().next_back(), Some("second"));
    /// let saved: Vec<&str> = editor.history().collect();
    /// assert_eq!(saved, ["first", "second"]);
    /// ```
    pub fn history(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator + '_ {
        self.history.iter().map(String::as_str)
    }

    /// Moves the cursor, reporting whether it moved.
    #[inline]
    fn move_to(&mut self, pos: usize) -> bool {
        let moved = pos != self.cursor;
        self.cursor = pos;
        moved
    }

    fn insert_char(&mut self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        self.line.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        true
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let start = prev_unit(&self.line, self.cursor);
        self.line.replace_range(start..self.cursor, "");
        self.cursor = start;
        true
    }

    fn delete(&mut self) -> bool {
        let end = next_unit(&self.line, self.cursor);
        if end == self.cursor {
            return false;
        }
        self.line.replace_range(self.cursor..end, "");
        true
    }

    /// Moves `line[start..end]` into the kill buffer and leaves the cursor at
    /// `start`.
    fn kill_range(&mut self, start: usize, end: usize) -> bool {
        if start == end {
            return false;
        }
        self.kill.clear();
        self.kill.push_str(&self.line[start..end]);
        self.line.replace_range(start..end, "");
        self.cursor = start;
        true
    }

    fn yank(&mut self) -> bool {
        if self.kill.is_empty() {
            return false;
        }
        self.line.insert_str(self.cursor, &self.kill);
        self.cursor += self.kill.len();
        true
    }

    fn history_prev(&mut self) -> bool {
        let target = match self.browsing {
            None => match self.history.len().checked_sub(1) {
                Some(newest) => {
                    // Park the line being typed; `history_next` brings it back.
                    core::mem::swap(&mut self.line, &mut self.draft);
                    newest
                }
                None => return false,
            },
            Some(0) => return false,
            Some(index) => index - 1,
        };
        self.load(target);
        true
    }

    fn history_next(&mut self) -> bool {
        match self.browsing {
            None => false,
            Some(index) if index + 1 < self.history.len() => {
                self.load(index + 1);
                true
            }
            Some(_) => {
                self.restore_draft();
                true
            }
        }
    }

    /// Replaces the line with a copy of history entry `index`, cursor at the end.
    fn load(&mut self, index: usize) {
        self.line.clear();
        if let Some(entry) = self.history.get(index) {
            self.line.push_str(entry);
        }
        self.cursor = self.line.len();
        self.browsing = Some(index);
    }

    /// Ends history browsing by bringing back the line that was being typed
    /// when it started.
    fn restore_draft(&mut self) {
        if self.browsing.take().is_some() {
            core::mem::swap(&mut self.line, &mut self.draft);
            self.draft.clear();
            self.cursor = self.line.len();
        }
    }

    /// Ends history browsing and drops the parked draft: the recalled line,
    /// edited or not, is what the user chose to keep.
    fn forget_draft(&mut self) {
        self.browsing = None;
        self.draft.clear();
    }
}

/// Appends `line` to `history` under the editor's recording rules, recycling
/// the oldest entry's buffer once `capacity` is reached.
fn record(
    history: &mut VecDeque<String>,
    capacity: usize,
    line: &str,
    browsing: &mut Option<usize>,
) {
    if capacity == 0
        || line.trim().is_empty()
        || history.back().is_some_and(|newest| newest == line)
    {
        return;
    }
    let mut slot = if history.len() >= capacity {
        // Keep an in-progress browse pointing at the same entry after the shift.
        *browsing = browsing.map(|index| index.saturating_sub(1));
        history.pop_front().unwrap_or_default()
    } else {
        String::new()
    };
    slot.clear();
    slot.push_str(line);
    history.push_back(slot);
}

#[cfg(test)]
#[allow(unused_results)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Editor {
        let mut editor = Editor::new();
        assert!(editor.insert(text));
        editor
    }

    #[test]
    fn test_insert_char_advances_cursor_by_utf8_len() {
        let mut editor = Editor::new();
        assert!(editor.apply(Edit::Insert('é')));
        assert_eq!(editor.cursor(), 2);
        assert!(editor.apply(Edit::Insert('世')));
        assert_eq!(editor.cursor(), 5);
        assert_eq!(editor.line(), "é世");
    }

    #[test]
    fn test_insert_control_char_is_refused() {
        let mut editor = Editor::new();
        for c in ['\n', '\r', '\t', '\u{1b}', '\u{7f}', '\u{85}'] {
            assert!(!editor.apply(Edit::Insert(c)), "{c:?} was accepted");
        }
        assert_eq!(editor.line(), "");
    }

    #[test]
    fn test_insert_in_middle_keeps_tail() {
        let mut editor = typed("ac");
        assert!(editor.apply(Edit::Left));
        assert!(editor.apply(Edit::Insert('b')));
        assert_eq!(editor.line(), "abc");
        assert_eq!(editor.cursor(), 2);
    }

    #[test]
    fn test_insert_str_only_controls_returns_false() {
        let mut editor = Editor::new();
        assert!(!editor.insert("\n\t\r"));
        assert!(!editor.insert(""));
    }

    #[test]
    fn test_backspace_at_start_returns_false() {
        let mut editor = typed("x");
        assert!(editor.apply(Edit::Home));
        assert!(!editor.apply(Edit::Backspace));
        assert_eq!(editor.line(), "x");
    }

    #[test]
    fn test_backspace_removes_whole_combining_unit() {
        let mut editor = typed("ae\u{0301}");
        assert!(editor.apply(Edit::Backspace));
        assert_eq!(editor.line(), "a");
        assert_eq!(editor.cursor(), 1);
    }

    #[test]
    fn test_delete_at_end_returns_false() {
        let mut editor = typed("x");
        assert!(!editor.apply(Edit::Delete));
    }

    #[test]
    fn test_delete_removes_unit_under_cursor() {
        let mut editor = typed("世界");
        assert!(editor.apply(Edit::Home));
        assert!(editor.apply(Edit::Delete));
        assert_eq!(editor.line(), "界");
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn test_left_right_at_bounds_return_false() {
        let mut editor = typed("ab");
        assert!(!editor.apply(Edit::Right));
        assert!(!editor.apply(Edit::End));
        assert!(editor.apply(Edit::Home));
        assert!(!editor.apply(Edit::Left));
        assert!(!editor.apply(Edit::Home));
    }

    #[test]
    fn test_column_counts_wide_and_zero_width() {
        let editor = typed("a世e\u{0301}");
        assert_eq!(editor.column(), 4);
    }

    #[test]
    fn test_kill_to_end_then_yank_round_trips() {
        let mut editor = typed("abc def");
        assert!(editor.apply(Edit::WordLeft));
        assert!(editor.apply(Edit::KillToEnd));
        assert_eq!(editor.line(), "abc ");
        assert!(!editor.apply(Edit::KillToEnd)); // nothing after the cursor
        assert!(editor.apply(Edit::Yank));
        assert_eq!(editor.line(), "abc def");
    }

    #[test]
    fn test_empty_kill_keeps_previous_kill_buffer() {
        let mut editor = typed("one two");
        assert!(editor.apply(Edit::KillWordLeft));
        assert!(!editor.apply(Edit::KillWordRight)); // at end: kills nothing
        assert!(editor.apply(Edit::Yank));
        assert_eq!(editor.line(), "one two");
    }

    #[test]
    fn test_kill_word_right_from_start() {
        let mut editor = typed("  alpha beta");
        assert!(editor.apply(Edit::Home));
        assert!(editor.apply(Edit::KillWordRight));
        assert_eq!(editor.line(), " beta");
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn test_yank_with_empty_kill_buffer_returns_false() {
        let mut editor = Editor::new();
        assert!(!editor.apply(Edit::Yank));
    }

    #[test]
    fn test_submit_returns_line_and_resets() {
        let mut editor = typed("print 1");
        assert_eq!(editor.submit(), "print 1");
        assert_eq!(editor.line(), "");
        assert_eq!(editor.cursor(), 0);
        assert!(editor.history().eq(["print 1"]));
    }

    #[test]
    fn test_submit_skips_blank_and_duplicate() {
        let mut editor = Editor::new();
        for line in ["a", "a", "", "  ", "b", "a"] {
            editor.insert(line);
            editor.submit();
        }
        assert!(editor.history().eq(["a", "b", "a"]));
    }

    #[test]
    fn test_history_capacity_zero_records_nothing() {
        let mut editor = Editor::with_history(0);
        editor.insert("x");
        assert_eq!(editor.submit(), "x");
        assert_eq!(editor.history().len(), 0);
        assert!(!editor.apply(Edit::HistoryPrev));
    }

    #[test]
    fn test_add_history_strips_control_characters() {
        let mut editor = Editor::new();
        editor.add_history("a\tb\u{1b}[0m");
        editor.add_history("\n\t"); // nothing typable left: blank, skipped
        assert!(editor.history().eq(["ab[0m"]));
        assert!(editor.apply(Edit::HistoryPrev));
        assert!(!editor.line().contains(char::is_control));
    }

    #[test]
    fn test_history_ring_drops_oldest_at_capacity() {
        let mut editor = Editor::with_history(3);
        for line in ["1", "2", "3", "4", "5"] {
            editor.add_history(line);
        }
        assert!(editor.history().eq(["3", "4", "5"]));
    }

    #[test]
    fn test_history_prev_next_restores_draft() {
        let mut editor = Editor::new();
        editor.add_history("old");
        editor.add_history("new");
        editor.insert("draft");

        assert!(editor.apply(Edit::HistoryPrev));
        assert_eq!(editor.line(), "new");
        assert!(editor.apply(Edit::HistoryPrev));
        assert_eq!(editor.line(), "old");
        assert!(!editor.apply(Edit::HistoryPrev)); // oldest reached
        assert!(editor.apply(Edit::HistoryNext));
        assert_eq!(editor.line(), "new");
        assert!(editor.apply(Edit::HistoryNext));
        assert_eq!(editor.line(), "draft");
        assert_eq!(editor.cursor(), 5);
        assert!(!editor.apply(Edit::HistoryNext)); // not browsing any more
    }

    #[test]
    fn test_history_prev_on_empty_history_returns_false() {
        let mut editor = typed("x");
        assert!(!editor.apply(Edit::HistoryPrev));
        assert_eq!(editor.line(), "x");
    }

    #[test]
    fn test_editing_recalled_entry_leaves_history_intact() {
        let mut editor = Editor::new();
        editor.add_history("abc");
        assert!(editor.apply(Edit::HistoryPrev));
        assert!(editor.apply(Edit::Backspace));
        assert_eq!(editor.line(), "ab");
        assert!(editor.history().eq(["abc"]));
        assert_eq!(editor.submit(), "ab");
        assert!(editor.history().eq(["abc", "ab"]));
    }

    #[test]
    fn test_submit_while_browsing_submits_recalled_line() {
        let mut editor = Editor::new();
        editor.add_history("again");
        editor.insert("draft");
        assert!(editor.apply(Edit::HistoryPrev));
        assert_eq!(editor.submit(), "again");
        // The draft was discarded with the browse; the duplicate not recorded.
        assert_eq!(editor.line(), "");
        assert!(editor.history().eq(["again"]));
    }

    #[test]
    fn test_add_history_while_browsing_at_capacity_keeps_entry() {
        let mut editor = Editor::with_history(2);
        editor.add_history("a");
        editor.add_history("b");
        assert!(editor.apply(Edit::HistoryPrev)); // on "b" (index 1)
        editor.add_history("c"); // drops "a"; "b" is now index 0
        assert!(editor.apply(Edit::HistoryNext));
        assert_eq!(editor.line(), "c");
    }

    #[test]
    fn test_clear_ends_browse_and_empties_line() {
        let mut editor = Editor::new();
        editor.add_history("x");
        editor.insert("draft");
        assert!(editor.apply(Edit::HistoryPrev));
        assert!(editor.clear());
        assert_eq!(editor.line(), "");
        assert!(!editor.apply(Edit::HistoryNext));
    }

    #[test]
    fn test_submit_steady_state_reuses_buffers() {
        let mut editor = Editor::with_history(2);
        for line in ["aaaa", "bbbb", "cccc"] {
            editor.insert(line);
            editor.submit();
        }
        // Once the ring is full, a submit recycles the oldest slot.
        let before = editor
            .history()
            .map(str::as_ptr)
            .collect::<alloc::vec::Vec<_>>();
        editor.insert("dddd");
        editor.submit();
        let after = editor
            .history()
            .map(str::as_ptr)
            .collect::<alloc::vec::Vec<_>>();
        assert_eq!(after[0], before[1]);
        assert_eq!(after[1], before[0]);
    }
}
