//! The values exchanged between a session and its pipeline.

use source_lang::{SourceId, Span};

/// The pipeline's verdict on the pending input: finished, or waiting for more.
///
/// The pipeline closure passed to [`Session::feed`](crate::Session::feed)
/// returns a `Status`. `Complete` carries whatever the pipeline produced — a
/// parsed tree, an evaluated value, a list of diagnostics, or a `Result` of
/// those — and the session hands it straight back in
/// [`Feed::Complete`]. Because the check for completeness and the real work
/// are the same call, the pipeline parses each finished entry exactly once.
///
/// Return `Complete` for input that is *wrong* as well as input that is
/// right: a syntax error in the middle of a line will not be fixed by typing
/// more lines. Only input that ended too soon should be `Incomplete`; see
/// [`Input::is_incomplete`](crate::Input::is_incomplete) for the usual test.
///
/// # Examples
///
/// ```
/// use repl_lang::{Feed, Session, Status};
///
/// // A pipeline that treats a trailing backslash as a line continuation.
/// let pipeline = |input: repl_lang::Input<'_>| {
///     if input.text().trim_end().ends_with('\\') {
///         Status::Incomplete
///     } else {
///         Status::Complete(input.text().lines().count())
///     }
/// };
///
/// let mut session = Session::new();
/// assert!(matches!(session.feed("echo one \\", pipeline)?, Feed::Incomplete));
/// assert!(matches!(session.feed("two", pipeline)?, Feed::Complete { value: 2, .. }));
/// # Ok::<(), repl_lang::SessionError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status<T> {
    /// The input is a whole entry. The session commits it and returns the
    /// value in [`Feed::Complete`].
    Complete(T),
    /// The input stops partway through a construct. The session keeps it and
    /// calls the pipeline again with the next line appended.
    Incomplete,
}

/// The outcome of feeding one line to a [`Session`](crate::Session).
///
/// A host's read loop matches on it: evaluate and print on `Complete`, show a
/// continuation prompt on `Incomplete`, and simply prompt again on `Empty`.
///
/// # Examples
///
/// ```
/// use repl_lang::{Feed, Session, Status};
///
/// let mut session = Session::new();
/// let pipeline = |input: repl_lang::Input<'_>| {
///     let open = input.text().matches('{').count();
///     let close = input.text().matches('}').count();
///     if open > close { Status::Incomplete } else { Status::Complete(input.text().len()) }
/// };
///
/// let mut transcript = Vec::new();
/// for line in ["", "f {", "  x", "}"] {
///     let prompt = if session.is_pending() { "... " } else { ">>> " };
///     match session.feed(line, pipeline)? {
///         Feed::Complete { entry, value } => {
///             transcript.push(format!("{prompt}{line}  -> entry {} ({value} bytes)", entry.number()));
///         }
///         Feed::Incomplete => transcript.push(format!("{prompt}{line}")),
///         Feed::Empty => transcript.push(prompt.trim_end().to_owned()),
///     }
/// }
/// assert_eq!(transcript, [">>>", ">>> f {", "...   x", "... }  -> entry 1 (10 bytes)"]);
/// # Ok::<(), repl_lang::SessionError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feed<T> {
    /// The pipeline accepted the entry. It is now committed to the session's
    /// source map, so diagnostics carrying its spans render against
    /// [`Session::sources`](crate::Session::sources).
    Complete {
        /// Where the committed entry lives.
        entry: Entry,
        /// What the pipeline returned in [`Status::Complete`].
        value: T,
    },
    /// The pipeline needs more input. The line is kept, and the next one is
    /// appended to it.
    Incomplete,
    /// The line was blank and no entry was in progress, so there was nothing
    /// to evaluate. The pipeline was not called and no entry number was used.
    Empty,
}

/// A committed entry: its number and where its text lives in the session's
/// source map.
///
/// Returned in [`Feed::Complete`]. The text itself is stored once, in the
/// session; an `Entry` is a small `Copy` handle to it.
///
/// # Examples
///
/// ```
/// use repl_lang::{Feed, Session, Span, Status};
///
/// let mut session = Session::new();
/// session.feed("first", |_| Status::Complete(()))?;
/// let Feed::Complete { entry, .. } = session.feed("second", |_| Status::Complete(()))? else {
///     return Err("the pipeline always completes".into());
/// };
///
/// assert_eq!(entry.number(), 2);
/// assert_eq!(entry.span(), Span::new(6, 13));
/// let source = session.sources().source(entry.id()).ok_or("missing")?;
/// assert_eq!(source.name(), "<repl:2>");
/// assert_eq!(source.text(), "second\n");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    number: u32,
    id: SourceId,
    span: Span,
}

impl Entry {
    #[inline]
    pub(crate) const fn new(number: u32, id: SourceId, span: Span) -> Self {
        Self { number, id, span }
    }

    /// The entry's number, counting from `1` in the order entries were
    /// committed. It matches [`Input::number`](crate::Input::number) for the
    /// input that produced it and is the key for
    /// [`Session::entry`](crate::Session::entry).
    #[inline]
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// The entry's id in [`Session::sources`](crate::Session::sources).
    #[inline]
    #[must_use]
    pub const fn id(&self) -> SourceId {
        self.id
    }

    /// The global span of the entry's text, equal to the
    /// [`Input::span`](crate::Input::span) the pipeline saw.
    #[inline]
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}
