//! The view of pending input a session hands to the pipeline.

use diag_lang::{Diagnostic, Severity};
use lexer_lang::Cursor;
use source_lang::{BytePos, Span};

/// The pending text of one REPL entry, as the pipeline sees it.
///
/// Each time a line is fed to a [`Session`](crate::Session), the session calls
/// the pipeline with an `Input` holding *every* line of the entry so far —
/// the first line, then the first two, and so on until the pipeline answers
/// [`Status::Complete`](crate::Status::Complete). Lines are joined with `'\n'`
/// and the text always ends with one.
///
/// The input also carries its position in the session's
/// [`SourceMap`](source_lang::SourceMap): [`base`](Input::base) is the global
/// offset its first byte will occupy once the entry is committed. Lex with
/// [`cursor`](Input::cursor), or offset spans by `base` yourself, and every span
/// the pipeline produces — in tokens, in syntax trees, in diagnostics — is
/// already valid against [`Session::sources`](crate::Session::sources). There
/// is no second pass to rebase spans after the entry is accepted.
///
/// `Input` is a borrowed, `Copy` view; it lives only for the duration of the
/// pipeline call.
///
/// # Examples
///
/// ```
/// use repl_lang::{Feed, Session, Status};
///
/// let mut session = Session::new();
/// let feed = session.feed("1 + 2", |input| {
///     assert_eq!(input.text(), "1 + 2\n");
///     assert_eq!(input.number(), 1);
///     Status::Complete(input.text().len())
/// })?;
/// assert!(matches!(feed, Feed::Complete { value: 6, .. }));
/// # Ok::<(), repl_lang::SessionError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input<'a> {
    text: &'a str,
    base: u32,
    number: u32,
}

impl<'a> Input<'a> {
    /// Wraps pending text that the session has checked will fit at `base`.
    #[inline]
    pub(crate) const fn new(text: &'a str, base: u32, number: u32) -> Self {
        Self { text, base, number }
    }

    /// Every line of the entry so far, joined with `'\n'` and ending with one.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Feed, Session, Status};
    ///
    /// let mut session = Session::new();
    /// let mut seen = Vec::new();
    /// for line in ["(1 +", " 2)"] {
    ///     session.feed(line, |input| {
    ///         seen.push(input.text().to_owned());
    ///         if input.text().matches('(').count() > input.text().matches(')').count() {
    ///             Status::Incomplete
    ///         } else {
    ///             Status::Complete(())
    ///         }
    ///     })?;
    /// }
    /// assert_eq!(seen, ["(1 +\n", "(1 +\n 2)\n"]);
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn text(&self) -> &'a str {
        self.text
    }

    /// The global position the first byte of this entry occupies in the
    /// session's source map.
    ///
    /// Entries are laid out end to end, so the base of entry *n* is the total
    /// length of entries 1 through *n − 1*.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("abc", |_| Status::Complete(()))?; // occupies 0..4 ("abc\n")
    /// session.feed("x", |input| {
    ///     assert_eq!(input.base().to_u32(), 4);
    ///     Status::Complete(())
    /// })?;
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn base(&self) -> BytePos {
        BytePos::new(self.base)
    }

    /// The global span the whole entry will occupy: from
    /// [`base`](Input::base) to the end of [`text`](Input::text).
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Span, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("let x = 1", |input| {
    ///     assert_eq!(input.span(), Span::new(0, 10));
    ///     Status::Complete(())
    /// })?;
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn span(&self) -> Span {
        // The session admits text only if `base + len` fits in `u32`.
        Span::new(self.base, self.base + self.text.len() as u32)
    }

    /// The entry number this input will be committed as, counting from `1`.
    ///
    /// Useful for prompts (`In [3]:`) and for names the evaluator binds per
    /// entry (`_3`). The number does not advance while an entry is incomplete,
    /// and blank lines never consume one.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("a", |input| { assert_eq!(input.number(), 1); Status::Complete(()) })?;
    /// session.feed("b", |input| { assert_eq!(input.number(), 2); Status::Complete(()) })?;
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// A [`lexer_lang::Cursor`] over [`text`](Input::text), positioned at
    /// [`base`](Input::base).
    ///
    /// Every token span the cursor produces is a global span in the session's
    /// source map, so tokens, the syntax built from them, and any diagnostic
    /// pointing at them render correctly against
    /// [`Session::sources`](crate::Session::sources) once the entry is
    /// committed.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("first", |_| Status::Complete(()))?; // 0..6
    /// session.feed("ab", |input| {
    ///     let mut cursor = input.cursor();
    ///     cursor.eat_while(|c| c.is_alphabetic());
    ///     assert_eq!(cursor.lexeme(), "ab");
    ///     assert_eq!(cursor.token_span().start().to_u32(), 6); // global, not 0
    ///     Status::Complete(())
    /// })?;
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn cursor(&self) -> Cursor<'a> {
        Cursor::with_base(self.text, self.base)
    }

    /// Returns `true` if `errors` say the input ended too soon rather than that
    /// it is wrong — the signal to ask for another line.
    ///
    /// That is the case when there is at least one error-severity diagnostic,
    /// and every one of them points at the end of the input: its primary span
    /// starts at or after the last non-whitespace byte. A parser that reaches
    /// the end of `1 + (2 *` reports "expected expression" *at the end*, so the
    /// user simply has not finished typing; a parser that trips over `1 + )`
    /// reports the error at the `)`, and more lines will never fix that.
    /// Warnings, notes, and help messages are ignored.
    ///
    /// The rule matches how [`parser_lang`] reports a missing token at the end
    /// of input — an empty span just past the last token — and works with any
    /// parser that does the same. Errors a lexer reports where a construct
    /// *starts*, such as an unterminated string pointing at its opening quote,
    /// are not recognised; return [`Status::Incomplete`](crate::Status::Incomplete)
    /// for those directly.
    ///
    /// [`parser_lang`]: https://docs.rs/parser-lang
    ///
    /// # Examples
    ///
    /// ```
    /// use diag_lang::{Diagnostic, Label, Severity};
    /// use repl_lang::{Session, Span, Status};
    ///
    /// let error_at = |at: u32| {
    ///     Diagnostic::new(Severity::Error, "expected expression", Label::new(Span::empty(at), "here"))
    /// };
    ///
    /// let mut session = Session::new();
    /// session.feed("1 + (2 *", |input| {
    ///     // An error at the end (byte 8, before the trailing newline): incomplete.
    ///     assert!(input.is_incomplete(&[error_at(8)]));
    ///     // An error in the middle: the input is wrong, not unfinished.
    ///     assert!(!input.is_incomplete(&[error_at(2), error_at(8)]));
    ///     // No errors at all: complete.
    ///     assert!(!input.is_incomplete(&[]));
    ///     Status::Complete(())
    /// })?;
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[must_use]
    pub fn is_incomplete(&self, errors: &[Diagnostic]) -> bool {
        // Fits: `base + text.len()` fits in `u32`, and this is no longer.
        let end = self.base + self.text.trim_end().len() as u32;
        let mut any = false;
        for error in errors.iter().filter(|d| d.severity() == Severity::Error) {
            if error.primary().span().start().to_u32() < end {
                return false;
            }
            any = true;
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use diag_lang::Label;

    use super::*;

    fn diag(severity: Severity, at: u32) -> Diagnostic {
        Diagnostic::new(severity, "m", Label::new(Span::empty(at), "l"))
    }

    #[test]
    fn test_span_covers_text_from_base() {
        let input = Input::new("ab\n", 10, 1);
        assert_eq!(input.span(), Span::new(10, 13));
        assert_eq!(input.base(), BytePos::new(10));
    }

    #[test]
    fn test_cursor_positions_are_global() {
        let input = Input::new("xy\n", 40, 1);
        let mut cursor = input.cursor();
        assert_eq!(cursor.pos(), BytePos::new(40));
        assert_eq!(cursor.bump(), Some('x'));
        assert_eq!(cursor.pos(), BytePos::new(41));
    }

    #[test]
    fn test_is_incomplete_no_errors_is_false() {
        assert!(!Input::new("x\n", 0, 1).is_incomplete(&[]));
    }

    #[test]
    fn test_is_incomplete_error_at_trimmed_end_is_true() {
        // "1 +   \n" — the meaningful text ends at 3.
        let input = Input::new("1 +   \n", 0, 1);
        assert!(input.is_incomplete(&[diag(Severity::Error, 3)]));
        assert!(input.is_incomplete(&[diag(Severity::Error, 7)]));
        assert!(!input.is_incomplete(&[diag(Severity::Error, 2)]));
    }

    #[test]
    fn test_is_incomplete_respects_base() {
        let input = Input::new("1 +\n", 100, 1);
        assert!(input.is_incomplete(&[diag(Severity::Error, 103)]));
        assert!(!input.is_incomplete(&[diag(Severity::Error, 3)]));
    }

    #[test]
    fn test_is_incomplete_ignores_non_errors() {
        let input = Input::new("1 +\n", 0, 1);
        assert!(!input.is_incomplete(&[diag(Severity::Warning, 3)]));
        assert!(input.is_incomplete(&[diag(Severity::Warning, 0), diag(Severity::Error, 3)]));
        assert!(!input.is_incomplete(&[diag(Severity::Note, 0), diag(Severity::Help, 1)]));
    }

    #[test]
    fn test_is_incomplete_any_early_error_wins() {
        let input = Input::new("a b c\n", 0, 1);
        let errors = [diag(Severity::Error, 5), diag(Severity::Error, 1)];
        assert!(!input.is_incomplete(&errors));
    }
}
