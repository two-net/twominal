//! How many cells a character takes (TERM-02).
//!
//! As many as libc's `wcwidth` gives it, as in xterm, because that is what programs count with:
//! a cursor they place after some text lands where the text ends only if the widths agree. The
//! widths are those of UAX #11, from `unicode-width`, with ambiguous characters narrow. Where
//! `unicode-width` differs from xterm's own `mk_wcwidth`, which follows libc, `mk_wcwidth` wins.
//!
//! Characters are counted one by one, never clustered: a combining mark takes no cell, and an
//! emoji sequence takes the cells of its parts, as xterm and `wcwidth` count them.

use unicode_width::UnicodeWidthChar;

/// How many cells `char` takes: 1, 2 for a wide character, or 0 for one that joins the
/// character before it. Inlined for plain text, which needs no table (PERF-06). The Renderer
/// lays out the text being composed with it, so that text takes the cells it will take once
/// it is printed.
#[inline]
pub fn width(char: char) -> usize {
    if (' '..'\u{7f}').contains(&char) {
        1
    } else {
        looked_up(char)
    }
}

/// [`width`], for a character that is not printable ASCII.
fn looked_up(char: char) -> usize {
    // Controls are never printed, so `None` is not met. The grid has no room for a character
    // wider than two cells.
    xterm(char).unwrap_or_else(|| char.width().map_or(1, |width| width.min(2)))
}

/// The width xterm's `mk_wcwidth` gives `char`, where `unicode-width` 0.2 gives another. Found
/// by comparing the two over every assigned character, with xterm patch 412. Compare them again
/// when `unicode-width` changes.
fn xterm(char: char) -> Option<usize> {
    Some(match char {
        // The soft hyphen, which libc shows.
        '\u{AD}' => 1,
        // Spacing vowel signs and marks, which `unicode-width` joins to the letter before them:
        // Bengali, Oriya, Tamil, Kannada, Malayalam, Sinhala, Balinese and others.
        '\u{9BE}'
        | '\u{9D7}'
        | '\u{B3E}'
        | '\u{B57}'
        | '\u{BBE}'
        | '\u{BD7}'
        | '\u{CC0}'
        | '\u{CC2}'
        | '\u{CC7}'..='\u{CC8}'
        | '\u{CCA}'..='\u{CCB}'
        | '\u{CD5}'..='\u{CD6}'
        | '\u{D3E}'
        | '\u{D57}'
        | '\u{DCF}'
        | '\u{DDF}'
        | '\u{1715}'
        | '\u{1734}'
        | '\u{1B35}'
        | '\u{1B3B}'
        | '\u{1B3D}'
        | '\u{1B43}'..='\u{1B44}'
        | '\u{1BAA}'
        | '\u{1BF2}'..='\u{1BF3}'
        | '\u{A953}'
        | '\u{A9C0}'
        | '\u{111C0}'
        | '\u{11235}'
        | '\u{1133E}'
        | '\u{1134D}'
        | '\u{11357}'
        | '\u{113B8}'
        | '\u{113C2}'
        | '\u{113C5}'
        | '\u{113C7}'..='\u{113C9}'
        | '\u{113CF}'
        | '\u{114B0}'
        | '\u{114BD}'
        | '\u{115AF}'
        | '\u{116B6}'
        | '\u{11930}'
        | '\u{1193D}'
        | '\u{11F41}'
        | '\u{1D165}'..='\u{1D166}'
        | '\u{1D16D}'..='\u{1D172}' => 1,
        // Letters and signs that `unicode-width` joins to the letter after them, or makes wider.
        '\u{D4E}'
        | '\u{17A4}'
        | '\u{17D8}'
        | '\u{A8FA}'
        | '\u{FF9E}'..='\u{FFA0}'
        | '\u{111C2}'..='\u{111C3}'
        | '\u{113D1}'
        | '\u{1193F}'
        | '\u{11941}'
        | '\u{11A84}'..='\u{11A89}'
        | '\u{11D46}'
        | '\u{11F02}' => 1,
        // Format characters and a joiner, which `unicode-width` shows.
        '\u{600}'..='\u{604}'
        | '\u{6DD}'
        | '\u{2D7F}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{110BD}'
        | '\u{110CD}'
        | '\u{13430}'..='\u{1343F}' => 0,
        // Wide characters that `unicode-width` joins to the character before them.
        '\u{302E}'..='\u{302F}' | '\u{3164}' | '\u{16FF0}'..='\u{16FF1}' => 2,
        _ => return None,
    })
}

/// Whether `char` is one of the format characters xterm discards rather than print: the zero
/// width space, the direction marks, embeddings, overrides and isolates, the word joiner, the
/// invisible operators, the deprecated format characters, the byte order mark and the
/// interlinear annotations. xterm discards ZWNJ and ZWJ too, but they are kept, because they
/// join emoji and shape scripts.
pub(crate) fn discarded(char: char) -> bool {
    matches!(
        char,
        '\u{200B}'
            | '\u{200E}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}
