//! Cursor-unit and word-boundary arithmetic over a line of text.
//!
//! The editor moves and deletes by *display unit*: a character with a visible
//! width followed by every zero-width character attached to it (combining marks,
//! variation selectors, joiners). This keeps `e` + COMBINING ACUTE ACCENT one
//! unit, so the cursor never lands between a letter and its accent and a
//! backspace removes what the user sees as one character. It is an
//! approximation of a grapheme cluster that needs no segmentation tables:
//! exact for combining sequences, and close for emoji sequences, whose
//! terminal rendering varies anyway.
//!
//! Every function takes and returns byte offsets that lie on `char` boundaries;
//! the editor relies on that to slice the line without panicking.

use unicode_lang::{char_width, is_xid_continue};

// The ASCII fast paths below skip the Unicode table lookups for the common
// case. They agree with the tables except for the tab: printable ASCII is one
// column wide, ASCII controls are zero, and ASCII `XID_Continue` is
// `[0-9A-Za-z_]`.
//
// Tabs and newlines reach the line only from a recalled multi-line history
// entry (typing and pasting refuse them). A tab counts as one column, so a
// host that draws it as one cell keeps the cursor in place; a newline starts a
// unit of its own and ends the line the cursor column is counted on.

/// The display width of `c` in terminal columns. A tab counts one.
#[inline]
fn width(c: char) -> usize {
    if c.is_ascii() {
        usize::from(!c.is_ascii_control() || c == '\t')
    } else {
        char_width(c)
    }
}

/// The display width of `text` in terminal columns: the sum of [`width`] over
/// its characters, with a whole-string shortcut for pure ASCII.
pub(crate) fn text_width(text: &str) -> usize {
    if text.is_ascii() {
        text.bytes()
            .filter(|&b| !b.is_ascii_control() || b == b'\t')
            .count()
    } else {
        text.chars().map(width).sum()
    }
}

/// Returns `true` if `c` starts a new display unit rather than attaching to the
/// one before it. A newline has no width but is a unit of its own, so the
/// cursor stops on both sides of it and one backspace removes only it.
#[inline]
fn starts_unit(c: char) -> bool {
    width(c) != 0 || c == '\n'
}

/// Returns `true` if `c` belongs to a word for word motion and word kills:
/// letters, digits, connector punctuation such as `_`, and combining marks.
#[inline]
fn is_word(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        is_xid_continue(c)
    }
}

/// The byte offset just past the display unit that starts at `pos`, or `pos`
/// itself at the end of the line.
pub(crate) fn next_unit(line: &str, pos: usize) -> usize {
    let mut chars = line[pos..].char_indices();
    if chars.next().is_none() {
        return pos;
    }
    chars
        .find(|&(_, c)| starts_unit(c))
        .map_or(line.len(), |(offset, _)| pos + offset)
}

/// The byte offset where the display unit that ends at `pos` begins, or `0` at
/// the start of the line.
pub(crate) fn prev_unit(line: &str, pos: usize) -> usize {
    line[..pos]
        .char_indices()
        .rev()
        .find(|&(_, c)| starts_unit(c))
        .map_or(0, |(offset, _)| offset)
}

/// The base character of the display unit starting at `pos`.
#[inline]
fn unit_at(line: &str, pos: usize) -> Option<char> {
    line[pos..].chars().next()
}

/// The end of the next word at or after `pos`: skip separators, then the word.
pub(crate) fn word_right(line: &str, mut pos: usize) -> usize {
    while unit_at(line, pos).is_some_and(|c| !is_word(c)) {
        pos = next_unit(line, pos);
    }
    while unit_at(line, pos).is_some_and(is_word) {
        pos = next_unit(line, pos);
    }
    pos
}

/// The start of the word at or before `pos`: skip separators, then the word.
pub(crate) fn word_left(line: &str, mut pos: usize) -> usize {
    while pos > 0 {
        let start = prev_unit(line, pos);
        if unit_at(line, start).is_some_and(is_word) {
            break;
        }
        pos = start;
    }
    while pos > 0 {
        let start = prev_unit(line, pos);
        if !unit_at(line, start).is_some_and(is_word) {
            break;
        }
        pos = start;
    }
    pos
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use unicode_lang::str_width;

    use super::*;

    const ACUTE: char = '\u{0301}';

    #[test]
    fn test_next_unit_ascii_steps_one_byte() {
        assert_eq!(next_unit("abc", 0), 1);
        assert_eq!(next_unit("abc", 2), 3);
        assert_eq!(next_unit("abc", 3), 3);
    }

    #[test]
    fn test_next_unit_combining_mark_joins_base() {
        let line = format!("e{ACUTE}x");
        assert_eq!(next_unit(&line, 0), 3); // 'e' (1) + U+0301 (2)
        assert_eq!(next_unit(&line, 3), 4);
    }

    #[test]
    fn test_next_unit_multibyte_wide_char() {
        assert_eq!(next_unit("世界", 0), 3);
        assert_eq!(next_unit("世界", 3), 6);
    }

    #[test]
    fn test_prev_unit_combining_mark_joins_base() {
        let line = format!("ae{ACUTE}");
        assert_eq!(prev_unit(&line, line.len()), 1);
        assert_eq!(prev_unit(&line, 1), 0);
        assert_eq!(prev_unit(&line, 0), 0);
    }

    #[test]
    fn test_prev_unit_orphan_mark_at_start_goes_to_zero() {
        let line = format!("{ACUTE}{ACUTE}");
        assert_eq!(prev_unit(&line, line.len()), 0);
    }

    #[test]
    fn test_word_right_skips_separators_then_word() {
        let line = "let  value = 1";
        assert_eq!(word_right(line, 0), 3);
        assert_eq!(word_right(line, 3), 10);
        assert_eq!(word_right(line, 10), 14);
        assert_eq!(word_right(line, 14), 14);
    }

    #[test]
    fn test_word_left_skips_separators_then_word() {
        let line = "let  value = 1";
        assert_eq!(word_left(line, 14), 13);
        assert_eq!(word_left(line, 13), 5);
        assert_eq!(word_left(line, 5), 0);
        assert_eq!(word_left(line, 0), 0);
    }

    #[test]
    fn test_word_motion_treats_underscore_and_digits_as_word() {
        let line = "a_b9 + c";
        assert_eq!(word_right(line, 0), 4);
        assert_eq!(word_left(line, 4), 0);
    }

    #[test]
    fn test_word_motion_keeps_accented_word_whole() {
        let line = format!("caf{ACUTE}e! x");
        let end = word_right(&line, 0);
        assert_eq!(&line[..end], format!("caf{ACUTE}e"));
        assert_eq!(word_left(&line, end), 0);
    }

    #[test]
    fn test_ascii_fast_paths_agree_with_unicode_tables() {
        for c in (0_u8..=0x7f).map(char::from).filter(|&c| c != '\t') {
            assert_eq!(width(c), char_width(c), "width of {c:?}");
            assert_eq!(is_word(c), is_xid_continue(c), "word class of {c:?}");
        }
        let all: alloc::string::String = (0_u8..=0x7f).map(char::from).collect();
        // The tab is the one deliberate difference: one column, not zero.
        assert_eq!(text_width(&all), str_width(&all) + 1);
        assert_eq!(text_width("a世e\u{0301}"), str_width("a世e\u{0301}"));
        assert_eq!(text_width("a\tb"), 3);
        assert_eq!(text_width("é\t"), 2);
    }

    #[test]
    fn test_newline_and_tab_are_units_of_their_own() {
        let line = "a\n\u{0301}\tb";
        assert_eq!(next_unit(line, 0), 1); // `a`
        assert_eq!(next_unit(line, 1), 4); // the newline, with the orphan mark after it
        assert_eq!(next_unit(line, 4), 5); // the tab
        assert_eq!(prev_unit(line, 5), 4);
        assert_eq!(prev_unit(line, 4), 1);
        assert_eq!(prev_unit(line, 1), 0);
        assert_eq!(word_right("ab\ncd", 0), 2);
        assert_eq!(word_right("ab\ncd", 2), 5);
        assert_eq!(word_left("ab\ncd", 5), 3);
    }

    #[test]
    fn test_word_motion_empty_line_stays_put() {
        assert_eq!(word_right("", 0), 0);
        assert_eq!(word_left("", 0), 0);
    }
}
