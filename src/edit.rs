//! The editing commands an [`Editor`](crate::Editor) understands.

/// One editing command for an [`Editor`](crate::Editor).
///
/// `Edit` is the boundary between the terminal and the editor. The host reads
/// key events however its platform delivers them — a raw-mode terminal library,
/// a GUI text field, a test script — and maps each one to an `Edit`; the editor
/// applies it to the line and reports whether anything changed. Keeping the
/// mapping on the host's side means the editor never touches a terminal and
/// any keymap (Emacs, Vi-style, custom) is a lookup table away.
///
/// Motions and deletions work in *display units*: a visible character together
/// with any zero-width characters attached to it, such as a combining accent.
/// The cursor never stops inside a unit. A *word* is a run of identifier
/// characters (Unicode `XID_Continue`: letters, digits, `_`, combining marks);
/// everything else separates words.
///
/// The usual Emacs/readline bindings are noted on each variant as a guide; they
/// are not built in.
///
/// The enum is `#[non_exhaustive]`, so new commands can arrive in a minor
/// release. Hosts build `Edit` values rather than matching on them, so this
/// costs nothing in practice.
///
/// # Examples
///
/// ```
/// use repl_lang::{Edit, Editor};
///
/// let mut editor = Editor::new();
/// for c in "print x".chars() {
///     editor.apply(Edit::Insert(c));
/// }
/// assert!(editor.apply(Edit::WordLeft));    // cursor before `x`
/// assert!(editor.apply(Edit::KillToStart)); // removes "print "
/// assert_eq!(editor.line(), "x");
/// assert!(editor.apply(Edit::Yank));        // and puts it back
/// assert_eq!(editor.line(), "print x");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Edit {
    /// Inserts a character at the cursor and moves the cursor past it.
    ///
    /// Control characters (`'\n'`, `'\t'`, escape, and the rest of the C0/C1
    /// ranges) are refused: they have no printable form on a single line, and
    /// a terminal key handler should turn them into commands instead.
    Insert(char),
    /// Deletes the display unit before the cursor. `Backspace`, `Ctrl-H`.
    Backspace,
    /// Deletes the display unit under the cursor. `Delete`, `Ctrl-D` on a
    /// non-empty line.
    Delete,
    /// Moves one display unit left. `←`, `Ctrl-B`.
    Left,
    /// Moves one display unit right. `→`, `Ctrl-F`.
    Right,
    /// Moves to the start of the current or previous word. `Ctrl-←`, `Alt-B`.
    WordLeft,
    /// Moves to the end of the current or next word. `Ctrl-→`, `Alt-F`.
    WordRight,
    /// Moves to the start of the line. `Home`, `Ctrl-A`.
    Home,
    /// Moves to the end of the line. `End`, `Ctrl-E`.
    End,
    /// Cuts everything before the cursor into the kill buffer. `Ctrl-U`.
    KillToStart,
    /// Cuts everything from the cursor to the end into the kill buffer.
    /// `Ctrl-K`.
    KillToEnd,
    /// Cuts the word before the cursor into the kill buffer. `Ctrl-W`,
    /// `Alt-Backspace`.
    KillWordLeft,
    /// Cuts the word after the cursor into the kill buffer. `Alt-D`.
    KillWordRight,
    /// Inserts the kill buffer at the cursor. `Ctrl-Y`.
    Yank,
    /// Replaces the line with the previous (older) history entry. The first
    /// step saves the line being typed so [`HistoryNext`](Edit::HistoryNext)
    /// can return to it. `↑`, `Ctrl-P`.
    HistoryPrev,
    /// Replaces the line with the next (newer) history entry, or restores the
    /// line that was being typed when browsing started. `↓`, `Ctrl-N`.
    HistoryNext,
}
