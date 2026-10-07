//! Why a session refused a line.

use core::fmt;

/// The reason a [`Session`](crate::Session) could not accept a line.
///
/// Both variants mean the pending entry was too large to keep, so the session
/// discards it before returning the error: the next line starts a fresh entry
/// and the session stays usable. Nothing already committed is affected. The
/// pipeline is never called with input that triggered an error.
///
/// The enum is `#[non_exhaustive]`; match it with a wildcard arm.
///
/// # Examples
///
/// ```
/// use repl_lang::{Session, SessionError, Status};
///
/// let mut session = Session::with_limit(8);
/// let err = session.feed("a line that is far too long", |_| Status::Complete(())).unwrap_err();
/// assert!(matches!(err, SessionError::TooLong { limit: 8, .. }));
/// assert!(!session.is_pending()); // the oversized entry was dropped
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionError {
    /// The entry, with this line added, would exceed the session's size limit.
    ///
    /// The limit bounds how much text one entry can accumulate — and so how
    /// much the pipeline re-reads on every continuation line — guarding
    /// against a runaway paste or a pipeline that never declares input
    /// complete. Report it to the user; raise the limit with
    /// [`Session::with_limit`](crate::Session::with_limit) if legitimate
    /// entries hit it.
    TooLong {
        /// Byte length the entry would have reached, including line terminators.
        len: usize,
        /// The session's limit, in bytes.
        limit: usize,
    },

    /// The session's source map has no room left for the entry.
    ///
    /// Committed entries share one 32-bit position space, so a session can hold
    /// about 4 GiB of entries in total. A shorter entry may still fit, but the
    /// session is effectively full: start a fresh [`Session`](crate::Session).
    SpaceExhausted {
        /// Byte length of the entry that did not fit.
        needed: u64,
        /// Bytes of position space that remained.
        available: u64,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len, limit } => write!(
                f,
                "entry too long: {len} bytes exceeds the session limit of {limit} bytes"
            ),
            Self::SpaceExhausted { needed, available } => write!(
                f,
                "session source space exhausted: entry needs {needed} bytes, \
                 {available} remain"
            ),
        }
    }
}

impl core::error::Error for SessionError {}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_display_too_long_names_both_sizes() {
        let text = SessionError::TooLong { len: 12, limit: 8 }.to_string();
        assert_eq!(
            text,
            "entry too long: 12 bytes exceeds the session limit of 8 bytes"
        );
    }

    #[test]
    fn test_display_space_exhausted_names_both_sizes() {
        let text = SessionError::SpaceExhausted {
            needed: 9,
            available: 2,
        }
        .to_string();
        assert_eq!(
            text,
            "session source space exhausted: entry needs 9 bytes, 2 remain"
        );
    }

    #[test]
    fn test_error_trait_is_implemented() {
        fn assert_error<E: core::error::Error + Send + Sync + 'static>() {}
        assert_error::<SessionError>();
    }
}
