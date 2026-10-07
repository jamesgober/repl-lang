//! Session state: pending input, committed entries, and the feed loop.

use alloc::{string::String, vec::Vec};

use source_lang::{SourceFile, SourceId, SourceMap, Span};

use crate::{Entry, Feed, Input, SessionError, Status};

/// The state of one REPL session: the entry being typed, and every entry
/// accepted so far.
///
/// A REPL reads a line, decides whether it finishes a piece of code, and if
/// not keeps reading. `Session` owns that decision loop without owning the
/// language. Each call to [`feed`](Session::feed) appends a line to the
/// pending entry and runs the caller's *pipeline* — lex, parse, evaluate,
/// whatever the language does — on all of it. The pipeline answers
/// [`Status::Complete`] with its result, or [`Status::Incomplete`] to ask for
/// another line.
///
/// A completed entry is committed to the session's [`SourceMap`] as a source
/// named `<repl:N>`, laid out after the entries before it in one global
/// position space. The pipeline sees that position before the entry is
/// committed (as [`Input::base`]), so the spans it records are final the
/// moment they are made. A diagnostic produced by entry 7 can point at a
/// definition in entry 2, and a renderer given [`sources`](Session::sources)
/// shows both lines with the right names.
///
/// The session does no I/O. Pair it with any line source: an [`Editor`]
/// driven by a terminal, `stdin` read line by line, a script, a test.
///
/// [`Editor`]: crate::Editor
///
/// # Cost
///
/// While an entry is incomplete the pipeline re-reads it in full for every
/// new line, so an entry of *k* lines is processed *k* times. Interactive
/// entries are short and this keeps the pipeline stateless. A host that
/// receives a large paste should feed it as one chunk: `feed` accepts text
/// containing newlines. [`with_limit`](Session::with_limit) caps entry size.
///
/// The pending buffer is reused across entries. Committing an entry makes one
/// exact-size copy of its text and its name into the source map; otherwise
/// feeding allocates only when the pending buffer grows past its previous
/// size.
///
/// # Examples
///
/// A REPL loop over a fixed script, with a pipeline that waits for balanced
/// parentheses and then reports how many lines the entry spanned:
///
/// ```
/// use repl_lang::{Feed, Session, Status};
///
/// let mut session = Session::new();
/// let mut results = Vec::new();
///
/// for line in ["(define (square x)", "  (* x x))", "", "(square 4)"] {
///     let feed = session.feed(line, |input| {
///         let depth: i32 = input.text().chars().map(|c| match c {
///             '(' => 1,
///             ')' => -1,
///             _ => 0,
///         }).sum();
///         if depth > 0 { Status::Incomplete } else { Status::Complete(input.text().lines().count()) }
///     })?;
///     if let Feed::Complete { entry, value } = feed {
///         results.push((entry.number(), value));
///     }
/// }
///
/// assert_eq!(results, [(1, 2), (2, 1)]);
/// assert_eq!(session.len(), 2);
/// assert_eq!(session.entry(1).map(|e| e.text()), Some("(define (square x)\n  (* x x))\n"));
/// # Ok::<(), repl_lang::SessionError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Session {
    /// One source per committed entry, in entry order.
    sources: SourceMap,
    /// The id of entry `n` is `ids[n - 1]`.
    ids: Vec<SourceId>,
    /// The entry being accumulated; every line ends with `'\n'`.
    pending: String,
    /// Reused buffer for building entry names.
    name: String,
    /// The global offset the next committed entry will start at.
    next_base: u32,
    /// Maximum byte length of one entry.
    limit: usize,
}

impl Default for Session {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// The entry size limit of [`Session::new`]: 1 MiB.
    pub const DEFAULT_LIMIT: usize = 1 << 20;

    /// Creates a session with no entries and the
    /// [`DEFAULT_LIMIT`](Session::DEFAULT_LIMIT) on entry size.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Session;
    ///
    /// let session = Session::new();
    /// assert!(session.is_empty());
    /// assert!(!session.is_pending());
    /// assert_eq!(session.limit(), Session::DEFAULT_LIMIT);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_limit(Self::DEFAULT_LIMIT)
    }

    /// Creates a session whose entries may grow to at most `limit` bytes,
    /// counting the `'\n'` that ends each line.
    ///
    /// A line that would take the pending entry past the limit is refused with
    /// [`SessionError::TooLong`], and the entry is discarded. The limit bounds
    /// memory held for one entry and the work the pipeline repeats on each
    /// continuation line, so lower it when input is untrusted.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Feed, Session, SessionError, Status};
    ///
    /// let mut session = Session::with_limit(16);
    /// let more = |_: repl_lang::Input<'_>| Status::<()>::Incomplete;
    ///
    /// assert!(matches!(session.feed("0123456789", more)?, Feed::Incomplete)); // 11 bytes
    /// let err = session.feed("abcdef", more).unwrap_err(); // would be 18
    /// assert_eq!(err, SessionError::TooLong { len: 18, limit: 16 });
    /// assert!(!session.is_pending());
    /// # Ok::<(), SessionError>(())
    /// ```
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            sources: SourceMap::new(),
            ids: Vec::new(),
            pending: String::new(),
            name: String::new(),
            next_base: 0,
            limit,
        }
    }

    /// Appends a line to the pending entry and runs `pipeline` on the whole
    /// entry.
    ///
    /// `line` is one line of input, with or without its line terminator; a
    /// single trailing `"\n"` or `"\r\n"` is removed and a `'\n'` appended, so
    /// lines from `read_line` and from `str::lines` behave the same. It may
    /// also be several lines at once, as from a paste.
    ///
    /// What happens next:
    ///
    /// - If no entry is pending and `line` is blank, nothing is evaluated:
    ///   the result is [`Feed::Empty`] and the pipeline is not called.
    /// - Otherwise the line joins the pending entry and the pipeline is called
    ///   with an [`Input`] over all of it.
    /// - On [`Status::Incomplete`] the entry stays pending and the result is
    ///   [`Feed::Incomplete`]. Blank lines inside an entry are passed through,
    ///   so a language can use one to end a block.
    /// - On [`Status::Complete`] the entry is committed as `<repl:N>` and the
    ///   result is [`Feed::Complete`] with the pipeline's value. The pending
    ///   buffer is cleared for the next entry.
    ///
    /// # Errors
    ///
    /// - [`SessionError::TooLong`] if the entry would exceed
    ///   [`limit`](Session::limit).
    /// - [`SessionError::SpaceExhausted`] if the session's source map cannot
    ///   hold the entry.
    ///
    /// In both cases the pending entry is discarded and the pipeline is not
    /// called, so the session is ready for a fresh entry.
    ///
    /// # Examples
    ///
    /// Feeding lines straight from `read_line`, terminators included:
    ///
    /// ```
    /// use repl_lang::{Feed, Session, Status};
    ///
    /// let stdin_lines = ["let x =\r\n", "  42\n"];
    /// let mut session = Session::new();
    /// let mut last = None;
    /// for raw in stdin_lines {
    ///     last = Some(session.feed(raw, |input| {
    ///         if input.text().trim_end().ends_with('=') {
    ///             Status::Incomplete
    ///         } else {
    ///             Status::Complete(input.text().to_owned())
    ///         }
    ///     })?);
    /// }
    /// let Some(Feed::Complete { value, .. }) = last else { return Err("incomplete".into()) };
    /// assert_eq!(value, "let x =\n  42\n");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// The pipeline can capture the interpreter state it needs, mutably:
    ///
    /// ```
    /// use repl_lang::{Feed, Session, Status};
    ///
    /// let mut total = 0_i64;
    /// let mut session = Session::new();
    /// for line in ["5", "10", "-3"] {
    ///     session.feed(line, |input| match input.text().trim().parse::<i64>() {
    ///         Ok(n) => { total += n; Status::Complete(Ok(total)) }
    ///         Err(e) => Status::Complete(Err(e)),
    ///     })?;
    /// }
    /// assert_eq!(total, 12);
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    pub fn feed<T, F>(&mut self, line: &str, pipeline: F) -> Result<Feed<T>, SessionError>
    where
        F: FnOnce(Input<'_>) -> Status<T>,
    {
        let line = strip_terminator(line);
        if self.pending.is_empty() && line.trim().is_empty() {
            return Ok(Feed::Empty);
        }
        self.admit(line)?;
        self.pending.push_str(line);
        self.pending.push('\n');

        let number = self.next_number();
        match pipeline(Input::new(&self.pending, self.next_base, number)) {
            Status::Incomplete => Ok(Feed::Incomplete),
            Status::Complete(value) => {
                let entry = self.commit(number)?;
                Ok(Feed::Complete { entry, value })
            }
        }
    }

    /// Discards the pending entry, returning `true` if there was one.
    ///
    /// This is the session half of an interrupt (`Ctrl-C`): drop the
    /// half-typed entry and go back to the primary prompt. Committed entries
    /// are unaffected and the next entry number is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("if x {", |_| Status::<()>::Incomplete)?;
    /// assert!(session.is_pending());
    /// assert!(session.cancel());
    /// assert!(!session.is_pending());
    /// assert!(!session.cancel());
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    pub fn cancel(&mut self) -> bool {
        let had = !self.pending.is_empty();
        self.pending.clear();
        had
    }

    /// Returns `true` while an entry is incomplete — when the next line
    /// continues it rather than starting a new one.
    ///
    /// Hosts use it to choose between the primary prompt and the continuation
    /// prompt.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// let prompt = |s: &Session| if s.is_pending() { "... " } else { ">>> " };
    /// assert_eq!(prompt(&session), ">>> ");
    /// session.feed("[1,", |_| Status::<()>::Incomplete)?;
    /// assert_eq!(prompt(&session), "... ");
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The text of the pending entry: every line fed since the last complete
    /// entry, each ending in `'\n'`. Empty when nothing is pending.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("a", |_| Status::<()>::Incomplete)?;
    /// session.feed("b", |_| Status::<()>::Incomplete)?;
    /// assert_eq!(session.pending(), "a\nb\n");
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn pending(&self) -> &str {
        &self.pending
    }

    /// The number of committed entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("one", |_| Status::Complete(()))?;
    /// session.feed("", |_| Status::Complete(()))?; // blank: not an entry
    /// assert_eq!(session.len(), 1);
    /// # Ok::<(), repl_lang::SessionError>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Returns `true` if no entry has been committed yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Session;
    ///
    /// assert!(Session::new().is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The committed entry with the given number (counting from `1`), or
    /// `None` if there is no such entry.
    ///
    /// The returned [`SourceFile`] gives the entry's name (`<repl:N>`), its
    /// text, and its global span — what a `:history` or `:show` command needs.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::{Session, Status};
    ///
    /// let mut session = Session::new();
    /// session.feed("x = 1", |_| Status::Complete(()))?;
    ///
    /// let first = session.entry(1).ok_or("missing")?;
    /// assert_eq!(first.name(), "<repl:1>");
    /// assert_eq!(first.text(), "x = 1\n");
    /// assert!(session.entry(0).is_none());
    /// assert!(session.entry(2).is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn entry(&self, number: u32) -> Option<&SourceFile> {
        let index = usize::try_from(number).ok()?.checked_sub(1)?;
        self.sources.source(*self.ids.get(index)?)
    }

    /// The source map holding every committed entry, one source per entry.
    ///
    /// Render diagnostics against it: any span the pipeline produced from an
    /// [`Input`] resolves to the right entry, line, and column.
    ///
    /// # Examples
    ///
    /// ```
    /// use diag_lang::{Diagnostic, Label, Renderer, Severity};
    /// use repl_lang::{Session, Span, Status};
    ///
    /// let mut session = Session::new();
    /// let diag = session.feed("let y = x + 1", |input| {
    ///     // Pretend `x` is unbound: point at it with a global span.
    ///     let at = input.base().to_u32() + 8;
    ///     Status::Complete(Diagnostic::new(
    ///         Severity::Error,
    ///         "cannot find `x`",
    ///         Label::new(Span::new(at, at + 1), "not found"),
    ///     ))
    /// })?;
    /// let repl_lang::Feed::Complete { value: diag, .. } = diag else { return Err("pending".into()) };
    ///
    /// let text = Renderer::new().render(&diag, session.sources());
    /// assert!(text.contains("<repl:1>:1:9"));
    /// assert!(text.contains("^ not found"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// The maximum byte length of one entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use repl_lang::Session;
    ///
    /// assert_eq!(Session::with_limit(4096).limit(), 4096);
    /// ```
    #[inline]
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Checks that appending `line` (plus its `'\n'`) keeps the entry within
    /// the size limit and the remaining position space. On failure the
    /// pending entry is discarded.
    fn admit(&mut self, line: &str) -> Result<(), SessionError> {
        let len = self
            .pending
            .len()
            .saturating_add(line.len())
            .saturating_add(1);
        let error = if len > self.limit {
            SessionError::TooLong {
                len,
                limit: self.limit,
            }
        } else {
            let available = u64::from(u32::MAX - self.next_base);
            let needed = len as u64;
            if needed <= available {
                return Ok(());
            }
            SessionError::SpaceExhausted { needed, available }
        };
        self.pending.clear();
        Err(error)
    }

    /// The number the pending entry will be committed as.
    #[inline]
    fn next_number(&self) -> u32 {
        // Every entry occupies at least two bytes of the 32-bit position space,
        // so the space runs out long before the count could reach `u32::MAX`;
        // saturating keeps that reasoning out of the arithmetic.
        u32::try_from(self.ids.len()).map_or(u32::MAX, |n| n.saturating_add(1))
    }

    /// Moves the pending entry into the source map as entry `number`.
    fn commit(&mut self, number: u32) -> Result<Entry, SessionError> {
        self.name.clear();
        self.name.push_str("<repl:");
        push_decimal(&mut self.name, number);
        self.name.push('>');

        let needed = self.pending.len() as u64;
        let added = self.sources.add(self.name.as_str(), self.pending.as_str());
        self.pending.clear();
        let id = added.map_err(|_| SessionError::SpaceExhausted {
            needed,
            available: u64::from(u32::MAX - self.next_base),
        })?;

        // `admit` checked that the entry fits after `next_base`.
        let len = needed as u32;
        let span = Span::new(self.next_base, self.next_base + len);
        self.next_base += len;
        self.ids.push(id);
        Ok(Entry::new(number, id, span))
    }
}

/// Removes one trailing `"\n"` or `"\r\n"`.
#[inline]
fn strip_terminator(line: &str) -> &str {
    match line.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => line,
    }
}

/// Appends the decimal digits of `n` without going through `core::fmt`.
fn push_decimal(out: &mut String, mut n: u32) {
    let mut digits = [0_u8; 10];
    let mut start = digits.len();
    loop {
        start -= 1;
        // `n % 10 < 10`, so the digit is ASCII.
        digits[start] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for &digit in &digits[start..] {
        out.push(char::from(digit));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, unused_results)]
mod tests {
    use alloc::{borrow::ToOwned, string::ToString};

    use super::*;

    fn complete(input: Input<'_>) -> Status<String> {
        Status::Complete(input.text().to_owned())
    }

    fn incomplete(_: Input<'_>) -> Status<String> {
        Status::Incomplete
    }

    #[test]
    fn test_feed_blank_line_with_nothing_pending_is_empty() {
        let mut session = Session::new();
        for blank in ["", "   ", "\n", "\r\n", "\t \n"] {
            let feed = session.feed(blank, |_| -> Status<()> { panic!("pipeline called") });
            assert_eq!(feed, Ok(Feed::Empty));
        }
        assert!(session.is_empty());
    }

    #[test]
    fn test_feed_complete_commits_entry() {
        let mut session = Session::new();
        let feed = session.feed("x = 1", complete).unwrap();
        let Feed::Complete { entry, value } = feed else {
            panic!("expected complete, got {feed:?}");
        };
        assert_eq!(value, "x = 1\n");
        assert_eq!(entry.number(), 1);
        assert_eq!(entry.span(), Span::new(0, 6));
        assert_eq!(session.len(), 1);
        assert!(!session.is_pending());
        let source = session.sources().source(entry.id()).unwrap();
        assert_eq!(source.name(), "<repl:1>");
        assert_eq!(source.text(), "x = 1\n");
        assert_eq!(source.span(), entry.span());
    }

    #[test]
    fn test_feed_incomplete_accumulates_lines() {
        let mut session = Session::new();
        assert_eq!(session.feed("a", incomplete), Ok(Feed::Incomplete));
        assert_eq!(session.feed("", incomplete), Ok(Feed::Incomplete));
        assert_eq!(session.feed("b", incomplete), Ok(Feed::Incomplete));
        assert_eq!(session.pending(), "a\n\nb\n");
        assert!(session.is_empty());
    }

    #[test]
    fn test_feed_blank_line_inside_entry_reaches_pipeline() {
        let mut session = Session::new();
        session.feed("def f():", incomplete).unwrap();
        let mut seen = String::new();
        session
            .feed("", |input| {
                seen.push_str(input.text());
                Status::Complete(())
            })
            .unwrap();
        assert_eq!(seen, "def f():\n\n");
    }

    #[test]
    fn test_feed_strips_one_terminator() {
        let mut session = Session::new();
        let texts: Vec<String> = ["a\n", "b\r\n", "c", "d\n\n", "e\r"]
            .iter()
            .map(|line| match session.feed(line, complete).unwrap() {
                Feed::Complete { value, .. } => value,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(texts, ["a\n", "b\n", "c\n", "d\n\n", "e\r\n"]);
    }

    #[test]
    fn test_feed_bases_are_contiguous() {
        let mut session = Session::new();
        let mut bases = Vec::new();
        for line in ["one", "two", "three"] {
            session
                .feed(line, |input| {
                    bases.push(input.base().to_u32());
                    Status::Complete(())
                })
                .unwrap();
        }
        assert_eq!(bases, [0, 4, 8]);
        assert_eq!(session.entry(3).unwrap().span(), Span::new(8, 14));
    }

    #[test]
    fn test_feed_number_stable_while_incomplete() {
        let mut session = Session::new();
        let mut numbers = Vec::new();
        for (line, done) in [("a", false), ("b", true), ("c", true)] {
            session
                .feed(line, |input| {
                    numbers.push(input.number());
                    if done {
                        Status::Complete(())
                    } else {
                        Status::Incomplete
                    }
                })
                .unwrap();
        }
        assert_eq!(numbers, [1, 1, 2]);
    }

    #[test]
    fn test_feed_too_long_discards_pending_and_skips_pipeline() {
        let mut session = Session::with_limit(5);
        session.feed("ab", incomplete).unwrap(); // 3 bytes
        let err = session
            .feed("cd", |_| -> Status<()> { panic!("pipeline called") })
            .unwrap_err();
        assert_eq!(err, SessionError::TooLong { len: 6, limit: 5 });
        assert!(!session.is_pending());
        // The session is usable again.
        assert!(matches!(
            session.feed("ok", complete),
            Ok(Feed::Complete { .. })
        ));
    }

    #[test]
    fn test_feed_exactly_at_limit_is_accepted() {
        let mut session = Session::with_limit(4);
        assert!(matches!(
            session.feed("abc", complete),
            Ok(Feed::Complete { .. })
        ));
    }

    #[test]
    fn test_feed_zero_limit_refuses_everything_but_blanks() {
        let mut session = Session::with_limit(0);
        assert_eq!(session.feed("", complete), Ok(Feed::Empty));
        assert!(matches!(
            session.feed("x", complete),
            Err(SessionError::TooLong { .. })
        ));
    }

    #[test]
    fn test_feed_space_exhausted_when_map_full() {
        let mut session = Session::with_limit(usize::MAX);
        session.next_base = u32::MAX - 3; // pretend 4 GiB of entries came before
        let err = session.feed("abcd", complete).unwrap_err();
        assert_eq!(
            err,
            SessionError::SpaceExhausted {
                needed: 5,
                available: 3
            }
        );
        assert!(!session.is_pending());
    }

    #[test]
    fn test_cancel_keeps_entry_number() {
        let mut session = Session::new();
        session.feed("x", incomplete).unwrap();
        assert!(session.cancel());
        let mut number = 0;
        session
            .feed("y", |input| {
                number = input.number();
                Status::Complete(())
            })
            .unwrap();
        assert_eq!(number, 1);
    }

    #[test]
    fn test_entry_out_of_range_is_none() {
        let mut session = Session::new();
        session.feed("x", complete).unwrap();
        assert!(session.entry(0).is_none());
        assert!(session.entry(2).is_none());
        assert!(session.entry(u32::MAX).is_none());
        assert_eq!(session.entry(1).unwrap().text(), "x\n");
    }

    #[test]
    fn test_push_decimal_formats_like_display() {
        for n in [0, 7, 10, 99, 100, 12_345, u32::MAX] {
            let mut out = String::new();
            push_decimal(&mut out, n);
            assert_eq!(out, n.to_string());
        }
    }

    #[test]
    fn test_session_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Session>();
        assert_send_sync::<crate::Editor>();
    }
}
