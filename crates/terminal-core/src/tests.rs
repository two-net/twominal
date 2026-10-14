//! The Terminal Core, bytes in and screen out (TERM-01, TERM-06, ARC-03, SEC-05). #17 adds the
//! conformance suite and fuzzing.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use crate::{Event, MouseTracking, Report, Size, TerminalCore};

/// A terminal of `cols` × `rows` that has interpreted `bytes`.
fn core(cols: u16, rows: u16, bytes: &[u8]) -> TerminalCore {
    let mut core = TerminalCore::new(Size { cols, rows }, None);
    core.interpret(bytes);
    core
}

/// The rows of the screen being shown, without their trailing blanks.
fn rows(core: &TerminalCore) -> Vec<String> {
    core.screen()
        .rows()
        .iter()
        .map(|row| row.text().trim_end().to_string())
        .collect()
}

fn cursor(core: &TerminalCore) -> (u16, u16) {
    let cursor = core.cursor();
    (cursor.row, cursor.col)
}

/// Which rows of the screen being shown wrap into the next.
fn wrapped(core: &TerminalCore) -> Vec<bool> {
    core.screen()
        .rows()
        .iter()
        .map(|row| row.wrapped())
        .collect()
}

/// The replies to what `bytes` asks.
fn replies(core: &mut TerminalCore, bytes: &[u8]) -> Vec<Vec<u8>> {
    core.interpret(bytes)
        .into_iter()
        .map(|event| match event {
            Event::Reply(bytes) => bytes,
            other => panic!("not a reply: {other:?}"),
        })
        .collect()
}

/// The reports in `bytes`, for a session whose nonce is `n1`.
fn reports(bytes: &[u8]) -> Vec<Report> {
    let mut core = TerminalCore::new(Size { cols: 20, rows: 3 }, Some("n1"));
    core.interpret(bytes)
        .into_iter()
        .map(|event| match event {
            Event::Report(report) => report,
            other => panic!("not a report: {other:?}"),
        })
        .collect()
}

/// Rows `1` to `count`, each holding its number.
fn numbered(count: usize) -> Vec<u8> {
    let lines: Vec<String> = (1..=count).map(|line| line.to_string()).collect();
    lines.join("\r\n").into_bytes()
}

// Printing and wrapping

#[test]
fn text_is_printed_at_the_cursor() {
    let core = core(10, 3, b"hello");
    assert_eq!(rows(&core), ["hello", "", ""]);
    assert_eq!(cursor(&core), (0, 5));
}

#[test]
fn a_full_row_waits_for_the_next_character_to_wrap() {
    // fish needs this: printing in the last column does not move to the next row (xenl).
    let core = core(10, 3, b"0123456789\r\nnext");
    assert_eq!(rows(&core), ["0123456789", "next", ""]);
    assert_eq!(wrapped(&core), [false, false, false]);

    let core = self::core(10, 3, b"0123456789");
    assert_eq!(cursor(&core), (0, 9));
}

#[test]
fn the_next_character_wraps_and_marks_the_row() {
    let core = core(10, 3, b"0123456789ab");
    assert_eq!(rows(&core), ["0123456789", "ab", ""]);
    assert_eq!(wrapped(&core), [true, false, false]);
    assert_eq!(cursor(&core), (1, 2));
}

#[test]
fn wrapping_on_the_last_row_scrolls() {
    let core = core(5, 2, b"abcdefghijklmno");
    assert_eq!(rows(&core), ["fghij", "klmno"]);
    assert_eq!(wrapped(&core), [true, false]);
}

#[test]
fn moving_the_cursor_cancels_a_pending_wrap() {
    let full = b"0123456789";
    for (moves, expected, at) in [
        // From the last column, as xterm does without reverse wrap.
        (&b"\x08x"[..], ["01234567x9", ""], (0, 9)),
        (b"\rx", ["x123456789", ""], (0, 1)),
        (b"\x1b[Cx", ["012345678x", ""], (0, 9)),
        (b"\x1b[Dx", ["01234567x9", ""], (0, 9)),
        (b"\nx", ["0123456789", "         x"], (1, 9)),
        (b"\x1b[Kx", ["012345678x", ""], (0, 9)),
    ] {
        let core = core(10, 2, &[&full[..], moves].concat());
        assert_eq!(rows(&core), expected, "{moves:?}");
        assert_eq!(cursor(&core), at, "{moves:?}");
    }
}

#[test]
fn a_tab_keeps_a_pending_wrap() {
    let core = core(10, 2, b"0123456789\tx");
    assert_eq!(rows(&core), ["0123456789", "x"]);
}

#[test]
fn without_autowrap_the_last_column_is_written_over() {
    let core = core(10, 2, b"\x1b[?7l0123456789abc");
    assert_eq!(rows(&core), ["012345678c", ""]);
    assert_eq!(cursor(&core), (0, 9));
}

#[test]
fn insert_mode_pushes_the_row_right() {
    let core = core(10, 1, b"abcdefghij\r\x1b[4hXY");
    assert_eq!(rows(&core), ["XYabcdefgh"]);
    let core = self::core(10, 1, b"abcdefghij\r\x1b[4h\x1b[4lXY");
    assert_eq!(rows(&core), ["XYcdefghij"]);
}

#[test]
fn rep_repeats_the_last_character() {
    assert_eq!(rows(&core(10, 1, b"a\x1b[3b")), ["aaaa"]);
    assert_eq!(rows(&core(10, 1, b"\x1b(0q\x1b[2b")), ["───"]);
    // Only straight after a character.
    assert_eq!(rows(&core(10, 1, b"a\r\x1b[3b")), ["a"]);
    assert_eq!(rows(&core(10, 1, b"\x1b[3b")), [""]);
    assert_eq!(rows(&core(10, 1, b"a\x1b[?5b\x1b[3b")), ["a"]);
    let ignored = format!("a\x1b[{}b\x1b[3b", "1;".repeat(40));
    assert_eq!(rows(&core(10, 1, ignored.as_bytes())), ["a"]);
    // No more than a screenful, however many are asked for.
    let core = core(5, 2, b"a\x1b[65535b");
    assert_eq!(rows(&core), ["aaaaa", "a"]);
    // In insert mode, as if each were printed in turn.
    let core = self::core(6, 2, b"wxyz\r\x1b[4ha\x1b[2b");
    assert_eq!(rows(&core), ["aaawxy", ""]);
    let core = self::core(4, 3, b"\x1b[3;1Hxy\x1b[1;1H\x1b[4ha\x1b[5b");
    assert_eq!(rows(&core), ["aaaa", "aa", "xy"]);
}

#[test]
fn rep_is_the_character_printed_again() {
    let text = b"abcdefghij\r\nklmnopqrst\r\nuvwxyz\x1b[2;3H";
    for modes in [
        &b""[..],
        b"\x1b[4h",
        b"\x1b[?7l",
        b"\x1b[4h\x1b[?7l",
        b"\x1b[2;3r",
    ] {
        for count in [1, 2, 7, 8, 9, 23, 39] {
            let prefix = [&text[..], modes].concat();
            let repeated = core(
                8,
                5,
                &[&prefix[..], format!("a\x1b[{count}b").as_bytes()].concat(),
            );
            let printed = core(
                8,
                5,
                &[&prefix[..], "a".repeat(count + 1).as_bytes()].concat(),
            );
            assert!(repeated.terminal == printed.terminal, "{modes:?} {count}");
        }
    }
}

// C0 controls and tabs

#[test]
fn line_feeds_keep_the_column_unless_in_newline_mode() {
    for feed in [b'\n', 0x0b, 0x0c] {
        let core = core(10, 3, &[b'a', b'b', feed, b'c', b'd']);
        assert_eq!(rows(&core), ["ab", "  cd", ""]);
    }
    let core = core(10, 3, b"\x1b[20hab\ncd");
    assert_eq!(rows(&core), ["ab", "cd", ""]);
    assert!(core.modes().newline);
}

#[test]
fn backspace_stops_at_the_first_column() {
    let core = core(10, 1, b"\x08\x08ab\x08\x08\x08c");
    assert_eq!(rows(&core), ["cb"]);
}

#[test]
fn tabs_stop_every_eight_columns_and_at_the_last() {
    let core = core(20, 1, b"\tx\t\t\ty");
    assert_eq!(rows(&core), ["        x          y"]);
}

#[test]
fn tab_stops_can_be_set_and_cleared() {
    // HTS at column 5, then TBC 3 clears every stop.
    let core = core(20, 2, b"\x1b[5G\x1bH\r\tx\r\n\x1b[3g\ty");
    assert_eq!(rows(&core), ["    x", "                   y"]);
    // TBC 0 clears the stop under the cursor.
    let core = self::core(20, 1, b"\x1b[9G\x1b[g\r\tx");
    assert_eq!(rows(&core), ["                x"]);
    // DECST8C puts them back.
    let core = self::core(20, 1, b"\x1b[3g\x1b[?5W\tx");
    assert_eq!(rows(&core), ["        x"]);
}

#[test]
fn cht_and_cbt_move_by_tab_stops() {
    assert_eq!(cursor(&core(80, 1, b"\x1b[2I")), (0, 16));
    assert_eq!(cursor(&core(80, 1, b"\x1b[20G\x1b[Z")), (0, 16));
    assert_eq!(cursor(&core(80, 1, b"\x1b[20G\x1b[2Z")), (0, 8));
    assert_eq!(cursor(&core(80, 1, b"\x1b[20G\x1b[99Z")), (0, 0));
    assert_eq!(cursor(&core(80, 1, b"\x1b[99I")), (0, 79));
}

// Moving the cursor

#[test]
fn cup_counts_from_one_and_stays_on_the_screen() {
    for (sequence, at) in [
        (&b"\x1b[3;5H"[..], (2, 4)),
        (b"\x1b[3;5f", (2, 4)),
        (b"\x1b[3;5H\x1b[H", (0, 0)),
        (b"\x1b[0;0H", (0, 0)),
        (b"\x1b[99;99H", (9, 19)),
        (b"\x1b[65535;65535H", (9, 19)),
    ] {
        assert_eq!(cursor(&core(20, 10, sequence)), at, "{sequence:?}");
    }
}

#[test]
fn relative_moves_stop_at_the_edges() {
    for (sequence, at) in [
        (&b"\x1b[5;5H\x1b[2A"[..], (2, 4)),
        (b"\x1b[5;5H\x1b[99A", (0, 4)),
        (b"\x1b[5;5H\x1b[B", (5, 4)),
        (b"\x1b[5;5H\x1b[99B", (9, 4)),
        (b"\x1b[5;5H\x1b[99e", (9, 4)),
        (b"\x1b[5;5H\x1b[3C", (4, 7)),
        (b"\x1b[5;5H\x1b[99C", (4, 19)),
        (b"\x1b[5;5H\x1b[99a", (4, 19)),
        (b"\x1b[5;5H\x1b[2D", (4, 2)),
        (b"\x1b[5;5H\x1b[99D", (4, 0)),
        (b"\x1b[5;5H\x1b[2E", (6, 0)),
        (b"\x1b[5;5H\x1b[2F", (2, 0)),
        (b"\x1b[5;5H\x1b[9G", (4, 8)),
        (b"\x1b[5;5H\x1b[9`", (4, 8)),
        (b"\x1b[5;5H\x1b[9d", (8, 4)),
    ] {
        assert_eq!(cursor(&core(20, 10, sequence)), at, "{sequence:?}");
    }
}

#[test]
fn cuu_and_cud_stop_at_the_region_they_start_in() {
    // The region is rows 3 to 6, counted from 1.
    for (moves, at) in [
        (&b"\x1b[5H\x1b[9A"[..], 2),
        (b"\x1b[5H\x1b[9B", 5),
        (b"\x1b[1H\x1b[9B", 5),
        (b"\x1b[8H\x1b[9A", 2),
        (b"\x1b[8H\x1b[9B", 9),
        (b"\x1b[2H\x1b[9A", 0),
    ] {
        let core = core(10, 10, &[&b"\x1b[3;6r"[..], moves].concat());
        assert_eq!(cursor(&core).0, at, "{moves:?}");
    }
}

// The scrolling region

#[test]
fn decstbm_sets_the_region_and_homes_the_cursor() {
    let mut core = core(5, 5, &numbered(5));
    core.interpret(b"\x1b[2;4r");
    assert_eq!(cursor(&core), (0, 0));
    core.interpret(b"\x1b[4H\nx");
    assert_eq!(rows(&core), ["1", "3", "4", "x", "5"]);
}

#[test]
fn a_region_of_less_than_two_rows_is_ignored() {
    for region in [&b"\x1b[3;3r"[..], b"\x1b[4;2r", b"\x1b[9;99r"] {
        let mut core = core(5, 5, &numbered(5));
        core.interpret(&[&b"\x1b[3;3H"[..], region].concat());
        assert_eq!(cursor(&core), (2, 2), "{region:?}");
        // Still the whole screen.
        core.interpret(b"\x1b[5H\n");
        assert_eq!(rows(&core), ["2", "3", "4", "5", ""], "{region:?}");
    }
}

#[test]
fn a_region_past_the_bottom_ends_at_the_bottom() {
    let mut core = core(5, 5, &numbered(5));
    core.interpret(b"\x1b[2;99r\x1b[5H\n");
    assert_eq!(rows(&core), ["1", "3", "4", "5", ""]);
}

#[test]
fn index_scrolls_only_at_the_bottom_of_the_region() {
    // At the bottom of the region: the region scrolls.
    let mut core = core(5, 6, &numbered(6));
    core.interpret(b"\x1b[2;4r\x1b[4H\x1bD");
    assert_eq!(rows(&core), ["1", "3", "4", "", "5", "6"]);
    assert_eq!(cursor(&core), (3, 0));

    // Below the region on the last row: nothing scrolls.
    let mut core = self::core(5, 6, &numbered(6));
    core.interpret(b"\x1b[2;4r\x1b[6H\x1bD\n");
    assert_eq!(rows(&core), ["1", "2", "3", "4", "5", "6"]);
    assert_eq!(cursor(&core), (5, 0));

    // NEL is a carriage return as well.
    let core = self::core(5, 3, b"ab\x1bEc");
    assert_eq!(rows(&core), ["ab", "c", ""]);
}

#[test]
fn reverse_index_scrolls_only_at_the_top_of_the_region() {
    let mut core = core(5, 6, &numbered(6));
    core.interpret(b"\x1b[2;4r\x1b[2H\x1bM");
    assert_eq!(rows(&core), ["1", "", "2", "3", "5", "6"]);
    assert_eq!(cursor(&core), (1, 0));

    // Above the region on the first row: nothing scrolls.
    let mut core = self::core(5, 6, &numbered(6));
    core.interpret(b"\x1b[2;4r\x1b[1H\x1bM");
    assert_eq!(rows(&core), ["1", "2", "3", "4", "5", "6"]);
    assert_eq!(cursor(&core), (0, 0));
}

#[test]
fn origin_mode_counts_rows_from_the_region() {
    let mut core = core(10, 10, b"\x1b[3;6r\x1b[?6h");
    assert_eq!(cursor(&core), (2, 0));
    core.interpret(b"\x1b[2;3H");
    assert_eq!(cursor(&core), (3, 2));
    core.interpret(b"\x1b[99H");
    assert_eq!(cursor(&core), (5, 0));
    assert_eq!(replies(&mut core, b"\x1b[6n"), [b"\x1b[4;1R"]);
    core.interpret(b"\x1b[2d");
    assert_eq!(cursor(&core), (3, 0));
    core.interpret(b"\x1b[?6l");
    assert_eq!(cursor(&core), (0, 0));
}

// Erasing, inserting and deleting

/// Three rows of four letters, with the cursor on the `f`.
const GRID: &[u8] = b"abcd\r\nefgh\r\nijkl\x1b[2;2H";

#[test]
fn ed_erases_below_above_or_all() {
    for (which, expected) in [
        (&b"\x1b[J"[..], ["abcd", "e", ""]),
        (b"\x1b[0J", ["abcd", "e", ""]),
        (b"\x1b[1J", ["", "  gh", "ijkl"]),
        (b"\x1b[2J", ["", "", ""]),
        // The scrollback is #26's.
        (b"\x1b[3J", ["abcd", "efgh", "ijkl"]),
        (b"\x1b[?2J", ["", "", ""]),
    ] {
        let core = core(4, 3, &[GRID, which].concat());
        assert_eq!(rows(&core), expected, "{which:?}");
        assert_eq!(cursor(&core), (1, 1), "{which:?}");
    }
}

#[test]
fn el_erases_within_the_row() {
    for (which, expected) in [
        (&b"\x1b[K"[..], ["abcd", "e", "ijkl"]),
        (b"\x1b[1K", ["abcd", "  gh", "ijkl"]),
        (b"\x1b[2K", ["abcd", "", "ijkl"]),
        (b"\x1b[?K", ["abcd", "e", "ijkl"]),
    ] {
        let core = core(4, 3, &[GRID, which].concat());
        assert_eq!(rows(&core), expected, "{which:?}");
        assert_eq!(cursor(&core), (1, 1), "{which:?}");
    }
}

#[test]
fn erasing_to_the_end_of_a_row_ends_its_wrap() {
    let core = core(4, 2, b"abcdef\x1b[1;3H\x1b[K");
    assert_eq!(wrapped(&core), [false, false]);
    let core = self::core(4, 2, b"abcdef\x1b[1;3H\x1b[1K");
    assert_eq!(wrapped(&core), [true, false]);
}

#[test]
fn characters_are_erased_inserted_and_deleted_at_the_cursor() {
    for (edit, expected) in [
        (&b"\x1b[2X"[..], "ab  efgh"),
        (b"\x1b[X", "ab defgh"),
        (b"\x1b[999X", "ab"),
        (b"\x1b[2@", "ab  cdefgh"),
        (b"\x1b[999@", "ab"),
        (b"\x1b[2P", "abefgh"),
        (b"\x1b[999P", "ab"),
    ] {
        let core = core(10, 1, &[&b"abcdefgh\x1b[3G"[..], edit].concat());
        assert_eq!(rows(&core), [expected], "{edit:?}");
        assert_eq!(cursor(&core), (0, 2), "{edit:?}");
    }
}

#[test]
fn il_and_dl_act_inside_the_region_only() {
    for (edit, expected) in [
        (&b"\x1b[3H\x1b[L"[..], ["1", "2", "", "3", "5"]),
        (b"\x1b[3H\x1b[99L", ["1", "2", "", "", "5"]),
        (b"\x1b[3H\x1b[M", ["1", "2", "4", "", "5"]),
        (b"\x1b[2H\x1b[99M", ["1", "", "", "", "5"]),
        // Outside the region nothing changes.
        (b"\x1b[5H\x1b[L", ["1", "2", "3", "4", "5"]),
        (b"\x1b[1H\x1b[M", ["1", "2", "3", "4", "5"]),
    ] {
        let mut core = core(5, 5, &numbered(5));
        core.interpret(&[&b"\x1b[2;4r"[..], edit].concat());
        assert_eq!(rows(&core), expected, "{edit:?}");
    }
    // The cursor goes to the first column.
    let mut core = core(5, 5, &numbered(5));
    core.interpret(b"\x1b[3;3H\x1b[L");
    assert_eq!(cursor(&core), (2, 0));
}

#[test]
fn su_and_sd_scroll_the_region() {
    for (edit, expected) in [
        (&b"\x1b[S"[..], ["1", "3", "4", "", "5"]),
        (b"\x1b[2T", ["1", "", "", "2", "5"]),
        (b"\x1b[99S", ["1", "", "", "", "5"]),
        // With five parameters it is xterm's highlight mouse tracking.
        (b"\x1b[1;2;3;4;5T", ["1", "2", "3", "4", "5"]),
    ] {
        let mut core = core(5, 5, &numbered(5));
        core.interpret(&[&b"\x1b[2;4r\x1b[3;3H"[..], edit].concat());
        assert_eq!(rows(&core), expected, "{edit:?}");
        assert_eq!(cursor(&core), (2, 2), "{edit:?}");
    }
}

#[test]
fn rows_keep_their_wrap_when_they_scroll() {
    let core = core(4, 3, b"\r\nabcdef\x1b[3H\n");
    assert_eq!(rows(&core), ["abcd", "ef", ""]);
    assert_eq!(wrapped(&core), [true, false, false]);
    let core = self::core(4, 3, b"abcdef\x1b[H\x1b[L");
    assert_eq!(rows(&core), ["", "abcd", "ef"]);
    assert_eq!(wrapped(&core), [false, true, false]);
}

// Saving and restoring the cursor

#[test]
fn decrc_restores_what_decsc_saved() {
    // The position and the character sets.
    let core = core(10, 5, b"\x1b[3;4H\x1b(0\x1b7\x1b[H\x1b(B\x1b8q");
    assert_eq!(rows(&core), ["", "", "   ─", "", ""]);
    // And origin mode.
    let core = self::core(10, 5, b"\x1b[2;4r\x1b[?6h\x1b7\x1b[?6l\x1b8\x1b[H");
    assert_eq!(cursor(&core), (1, 0));
}

#[test]
fn decrc_restores_a_pending_wrap() {
    let core = core(10, 2, b"0123456789\x1b7\x1b[2;1H\x1b8x");
    assert_eq!(rows(&core), ["0123456789", "x"]);
}

#[test]
fn decrc_with_nothing_saved_goes_home() {
    assert_eq!(cursor(&core(10, 5, b"\x1b[3;3H\x1b8")), (0, 0));
}

#[test]
fn csi_s_and_csi_u_save_and_restore_the_cursor() {
    assert_eq!(cursor(&core(10, 5, b"\x1b[2;3H\x1b[s\x1b[H\x1b[u")), (1, 2));
    // With a private marker they are other sequences: XTSAVE, XTRESTORE and the kitty keyboard
    // protocol.
    for sequence in [
        &b"\x1b[?u"[..],
        b"\x1b[?1u",
        b"\x1b[=5u",
        b"\x1b[>1u",
        b"\x1b[<u",
    ] {
        let core = core(
            10,
            5,
            &[&b"\x1b[2;3H\x1b[s\x1b[4;4H"[..], sequence].concat(),
        );
        assert_eq!(cursor(&core), (3, 3), "{sequence:?}");
    }
}

#[test]
fn each_screen_has_a_saved_cursor_of_its_own() {
    let core = core(
        10,
        5,
        b"\x1b[2;2H\x1b7\x1b[?47h\x1b[4;4H\x1b7\x1b[?47l\x1b8",
    );
    assert_eq!(cursor(&core), (1, 1));
    let core = self::core(
        10,
        5,
        b"\x1b[2;2H\x1b7\x1b[?47h\x1b[4;4H\x1b7\x1b[?47l\x1b[?47h\x1b[H\x1b8",
    );
    assert_eq!(cursor(&core), (3, 3));
}

// The alternate screen

#[test]
fn mode_1049_saves_the_cursor_and_shows_a_clear_alternate_screen() {
    let mut core = core(10, 3, b"before\r\n");
    core.interpret(b"\x1b[?1049h");
    assert!(core.modes().alternate_screen);
    assert_eq!(rows(&core), ["", "", ""]);
    // The cursor stays where it was.
    assert_eq!(cursor(&core), (1, 0));

    core.interpret(b"\x1b[Hvim\x1b[3;5H");
    assert_eq!(rows(&core), ["vim", "", ""]);
    core.interpret(b"\x1b[?1049l");
    assert!(!core.modes().alternate_screen);
    assert_eq!(rows(&core), ["before", "", ""]);
    assert_eq!(cursor(&core), (1, 0));

    // Each time it is clear.
    core.interpret(b"\x1b[?1049h");
    assert_eq!(rows(&core), ["", "", ""]);
}

#[test]
fn mode_47_switches_screens_and_clears_nothing() {
    let mut core = core(10, 2, b"one\x1b[?47h\x1b[Htwo");
    assert_eq!(rows(&core), ["two", ""]);
    core.interpret(b"\x1b[?47l");
    assert_eq!(rows(&core), ["one", ""]);
    assert_eq!(cursor(&core), (0, 3));
    core.interpret(b"\x1b[?47h");
    assert_eq!(rows(&core), ["two", ""]);
}

#[test]
fn mode_1047_clears_the_alternate_screen_on_the_way_back() {
    let mut core = core(10, 2, b"one\x1b[?1047h\x1b[Htwo\x1b[?1047l");
    assert_eq!(rows(&core), ["one", ""]);
    core.interpret(b"\x1b[?1047h");
    assert_eq!(rows(&core), ["", ""]);
}

#[test]
fn mode_1048_saves_and_restores_the_cursor() {
    assert_eq!(
        cursor(&core(10, 5, b"\x1b[2;2H\x1b[?1048h\x1b[H\x1b[?1048l")),
        (1, 1)
    );
}

// Resets

#[test]
fn ris_resets_everything_but_the_size() {
    let mut core = core(
        10,
        3,
        b"text\x1b[2;3r\x1b[?25l\x1b[?2004h\x1b[?1000h\x1b[3g\x1b(0\x1b[?1049hfull",
    );
    core.interpret(b"\x1bc");
    assert_eq!(rows(&core), ["", "", ""]);
    assert_eq!(cursor(&core), (0, 0));
    assert_eq!(core.modes(), &crate::Modes::default());
    assert_eq!(core.size(), Size { cols: 10, rows: 3 });
    // The region, the tab stops, the character sets and the other screen too.
    assert!(core.terminal == self::core(10, 3, b"").terminal);
}

#[test]
fn decstr_resets_modes_but_keeps_the_screen_and_autowrap_on() {
    let mut core = core(
        4,
        3,
        b"abc\x1b[?7l\x1b[?25l\x1b[4h\x1b[?1h\x1b=\x1b(0\x1b[2;3r\x1b[?6h\x1b7\x1b[!p",
    );
    assert_eq!(rows(&core), ["abc", "", ""]);
    assert_eq!(cursor(&core), (1, 0));
    assert!(core.modes().cursor_visible);
    assert!(!core.modes().application_cursor && !core.modes().application_keypad);
    assert_eq!(
        replies(&mut core, b"\x1b[4$p\x1b[?6$p\x1b[?7$p"),
        [&b"\x1b[4;2$y"[..], b"\x1b[?6;2$y", b"\x1b[?7;1$y"]
    );
    // Nothing is saved, so the cursor goes home. The characters are ASCII again, and autowrap
    // is on again (xterm, not VT510): `tput reset` sends DECSTR.
    core.interpret(b"\x1b8qxyzw");
    assert_eq!(rows(&core), ["qxyz", "w", ""]);
    // The scrolling region is the whole screen again.
    core.interpret(b"\x1b[3H\n");
    assert_eq!(rows(&core), ["w", "", ""]);
}

#[test]
fn decaln_fills_the_screen_with_e() {
    let core = core(3, 2, b"abcd\x1b[2;2H\x1b#8");
    assert_eq!(rows(&core), ["EEE", "EEE"]);
    assert_eq!(wrapped(&core), [false, false]);
    assert_eq!(cursor(&core), (0, 0));
}

// Character sets

#[test]
fn dec_special_graphics_draw_lines() {
    let core = core(10, 3, b"\x1b(0lqqk\r\nx  x\r\nmqqj\x1b(Bq");
    assert_eq!(rows(&core), ["┌──┐", "│  │", "└──┘q"]);
}

#[test]
fn so_and_si_shift_between_g0_and_g1() {
    let core = core(10, 1, b"\x1b)0q\x0eq\x0fq");
    assert_eq!(rows(&core), ["q─q"]);
    let core = self::core(10, 1, b"\x1b*0\x1b+A\x1bnq\x1bo#");
    assert_eq!(rows(&core), ["─£"]);
    let core = self::core(10, 1, b"\x1b(A#\x1b(B#\x1b(Z#");
    assert_eq!(rows(&core), ["£##"]);
}

// Modes

#[test]
fn modes_are_kept_for_the_rest_of_the_tab() {
    let mut core = core(
        10,
        2,
        b"\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h\x1b[?25l\x1b[20h",
    );
    let modes = *core.modes();
    assert!(modes.application_cursor && modes.application_keypad && modes.newline);
    assert!(modes.bracketed_paste && modes.focus && modes.mouse_sgr);
    assert!(!modes.cursor_visible);
    assert_eq!(modes.mouse, MouseTracking::Drag);

    // Turning off any mouse mode turns mouse reporting off.
    core.interpret(b"\x1b[?1000l\x1b>\x1b[?66h");
    assert_eq!(core.modes().mouse, MouseTracking::Off);
    assert!(core.modes().application_keypad);
    core.interpret(b"\x1b[?9;1003h");
    assert_eq!(core.modes().mouse, MouseTracking::Motion);
}

#[test]
fn modes_that_would_lock_echo_or_resize_are_ignored() {
    let core = core(10, 2, b"text\x1b[2h\x1b[12h\x1b[?3h\x1b[?3l\x1b[?5h");
    assert_eq!(rows(&core), ["text", ""]);
    assert_eq!(core.size(), Size { cols: 10, rows: 2 });
}

// Replies

#[test]
fn device_attributes_are_answered() {
    let mut core = core(10, 2, b"");
    assert_eq!(replies(&mut core, b"\x1b[c"), [b"\x1b[?62;22c"]);
    assert_eq!(replies(&mut core, b"\x1b[0c"), [b"\x1b[?62;22c"]);
    assert_eq!(replies(&mut core, b"\x1b[>c"), [b"\x1b[>1;10;0c"]);
    assert_eq!(replies(&mut core, b"\x1b[>0c"), [b"\x1b[>1;10;0c"]);
    assert_eq!(replies(&mut core, b"\x1bZ"), [b"\x1b[?62;22c"]);
    assert!(replies(&mut core, b"\x1b[1c\x1b[=c\x1b[>1c").is_empty());
}

#[test]
fn status_and_cursor_position_are_answered() {
    let mut core = core(10, 5, b"\x1b[3;7H");
    assert_eq!(replies(&mut core, b"\x1b[5n"), [b"\x1b[0n"]);
    assert_eq!(replies(&mut core, b"\x1b[6n"), [b"\x1b[3;7R"]);
    assert_eq!(replies(&mut core, b"\x1b[?6n"), [b"\x1b[?3;7R"]);
    assert!(replies(&mut core, b"\x1b[?5n\x1b[7n").is_empty());

    // With a wrap pending the cursor is in the last column.
    let mut core = self::core(10, 5, b"0123456789");
    assert_eq!(replies(&mut core, b"\x1b[6n"), [b"\x1b[1;10R"]);
}

#[test]
fn modes_are_reported() {
    let mut core = core(10, 2, b"");
    for (query, reply) in [
        (&b"\x1b[?2004$p"[..], &b"\x1b[?2004;2$y"[..]),
        (b"\x1b[?7$p", b"\x1b[?7;1$y"),
        (b"\x1b[?25$p", b"\x1b[?25;1$y"),
        (b"\x1b[?1049$p", b"\x1b[?1049;2$y"),
        (b"\x1b[?1048$p", b"\x1b[?1048;0$y"),
        (b"\x1b[?2026$p", b"\x1b[?2026;0$y"),
        (b"\x1b[?65535$p", b"\x1b[?65535;0$y"),
        (b"\x1b[4$p", b"\x1b[4;2$y"),
        (b"\x1b[20$p", b"\x1b[20;2$y"),
        (b"\x1b[2$p", b"\x1b[2;0$y"),
    ] {
        assert_eq!(replies(&mut core, query), [reply], "{query:?}");
    }
    core.interpret(b"\x1b[?2004h\x1b[?1049h\x1b[?1002h\x1b[4h");
    for (query, reply) in [
        (&b"\x1b[?2004$p"[..], &b"\x1b[?2004;1$y"[..]),
        (b"\x1b[?1049$p", b"\x1b[?1049;1$y"),
        (b"\x1b[?47$p", b"\x1b[?47;1$y"),
        (b"\x1b[?1002$p", b"\x1b[?1002;1$y"),
        (b"\x1b[?1000$p", b"\x1b[?1000;2$y"),
        (b"\x1b[4$p", b"\x1b[4;1$y"),
    ] {
        assert_eq!(replies(&mut core, query), [reply], "{query:?}");
    }
}

#[test]
fn each_query_gets_its_reply_in_order() {
    let mut core = core(10, 2, b"");
    assert_eq!(
        replies(&mut core, b"\x1b[c\x1b[5n\x1b[6n"),
        [&b"\x1b[?62;22c"[..], b"\x1b[0n", b"\x1b[1;1R"]
    );
}

#[test]
fn nothing_that_would_echo_the_programs_text_is_answered() {
    // SEC-05: a reply that held text the program chose would be typed into the shell.
    for query in [
        &b"\x1b[20t"[..],
        b"\x1b[21t",
        b"\x1b]2;rm -rf ~\x07\x1b[21t",
        b"\x1b[18t",
        b"\x1b[14t",
        b"\x1b[22;0t\x1b[23;0t",
        b"\x05",
        b"\x1bP$qm\x1b\\",
        b"\x1bP$q\"p\x1b\\",
        b"\x1bP+q696e646e\x1b\\",
        b"\x1b[>q",
        b"\x1b[>0q",
        b"\x1b]11;?\x07",
        b"\x1b]10;?\x1b\\",
        b"\x1b]4;1;?\x07",
        b"\x1b]52;c;?\x07",
        b"\x1b[?u",
    ] {
        let mut core = core(10, 2, b"");
        assert!(core.interpret(query).is_empty(), "{query:?}");
        assert_eq!(rows(&core), ["", ""], "{query:?}");
    }
}

// What is ignored

#[test]
fn sequences_left_to_other_features_change_nothing() {
    for sequence in [
        &b"\x1b[1;31m"[..],
        b"\x1b[38:2::255:0:0m",
        b"\x1b[>4;1m",
        b"\x1b[?4m",
        b"\x1b[?u",
        b"\x1b[=5u",
        b"\x1b[>1u",
        b"\x1b[ 2q",
        b"\x1b]0;title\x07",
        b"\x1b]7;file:///tmp\x1b\\",
        b"\x1b]8;;https://example.com\x07",
        b"\x1b]133;A\x07",
        b"\x1b]2222;n1;ready\x07",
        b"\x1b_Gf=24;AAAA\x1b\\",
        b"\x1b^private\x1b\\",
        b"\x1bXstring\x1b\\",
        b"\x1bPq#0;2;0;0;0#0~~\x1b\\",
        b"\x1b[5i",
        b"\x1b[?1;2;3S",
        b"\x1b[?1r",
        b"\x1b[1:2H",
        b"\x1bN\x1bO\x1b#3",
        b"\x07",
    ] {
        let core = core(10, 2, &[&b"a"[..], sequence, b"b"].concat());
        assert_eq!(rows(&core), ["ab", ""], "{sequence:?}");
        assert_eq!(cursor(&core), (0, 2), "{sequence:?}");
    }
}

#[test]
fn controls_that_are_not_characters_take_no_cell() {
    for text in [
        &b"a\x7fb"[..],
        b"a\xc2\x85b",
        b"a\xc2\x9b2Jb",
        b"a\x85b",
        b"a\x00b",
    ] {
        let core = core(10, 1, text);
        let row = &rows(&core)[0];
        assert!(
            row.starts_with('a') && row.ends_with('b'),
            "{text:?}: {row:?}"
        );
        assert!(
            !row.contains(|char: char| char.is_control()),
            "{text:?}: {row:?}"
        );
    }
    // A C1 control split in two is still not a character.
    let mut core = core(10, 1, b"a\xc2");
    core.interpret(b"\x85b");
    assert_eq!(rows(&core), ["ab"]);
    // What is not UTF-8 is shown as such.
    assert_eq!(rows(&self::core(10, 1, b"a\xffb")), ["a\u{fffd}b"]);
}

// Reports

#[test]
fn reports_that_carry_the_nonce_are_read() {
    let text = b"\x1b]2222;n1;cwd;/a%20b%3Bc%25d%C3%A9\x07\
                 \x1b]2222;n1;path;/usr/bin:/bin\x07\
                 \x1b]2222;n1;aliases;ll la\x07\
                 \x1b]2222;n1;functions;f%3Bx g\x07\
                 \x1b]2222;n1;cmdlets;Get-Item Set-Item\x07\
                 \x1b]2222;n1;ready\x07";
    assert_eq!(
        reports(text),
        [
            Report::Directory("/a b;c%dé".as_bytes().to_vec()),
            Report::SearchPath(b"/usr/bin:/bin".to_vec()),
            Report::Aliases(vec!["ll".to_string(), "la".to_string()]),
            Report::Functions(vec!["f;x".to_string(), "g".to_string()]),
            Report::Cmdlets(vec!["Get-Item".to_string(), "Set-Item".to_string()]),
            Report::Ready,
        ]
    );
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    core.interpret(&[&b"a"[..], text, b"b"].concat());
    assert_eq!(rows(&core), ["ab"]);
}

#[test]
fn a_report_keeps_its_place_among_the_replies() {
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    assert_eq!(
        core.interpret(b"\x1b[5n\x1b]2222;n1;ready\x07\x1b[5n"),
        [
            Event::Reply(b"\x1b[0n".to_vec()),
            Event::Report(Report::Ready),
            Event::Reply(b"\x1b[0n".to_vec()),
        ]
    );
}

#[test]
fn values_are_decoded_as_the_scripts_encode_them() {
    assert_eq!(
        reports(b"\x1b]2222;n1;cwd;%2f%2F\x07\x1b]2222;n1;path;\x07\x1b]2222;n1;aliases;\x07"),
        [
            Report::Directory(b"//".to_vec()),
            Report::SearchPath(Vec::new()),
            Report::Aliases(Vec::new()),
        ]
    );
    // A path is bytes, UTF-8 or not. A name that is not UTF-8 cannot be typed, so it is
    // left out.
    assert_eq!(
        reports(b"\x1b]2222;n1;cwd;/%FF\x07\x1b]2222;n1;functions;ok %FF  also\x07"),
        [
            Report::Directory(b"/\xff".to_vec()),
            Report::Functions(vec!["ok".to_string(), "also".to_string()]),
        ]
    );
}

#[test]
fn what_is_not_a_report_is_ignored() {
    for text in [
        // Another nonce, or another number.
        &b"\x1b]2222;n2;ready\x07"[..],
        b"\x1b]2222;n;ready\x07",
        b"\x1b]2222;;ready\x07",
        b"\x1b]2223;n1;ready\x07",
        // A key Twominal does not know, or one in the wrong shape.
        b"\x1b]2222;n1;title;x\x07",
        b"\x1b]2222;n1;ready;x\x07",
        b"\x1b]2222;n1;cwd\x07",
        b"\x1b]2222;n1;cwd;a;b\x07",
        b"\x1b]2222;n1\x07",
        // Not as the scripts write it.
        b"\x1b]2222;n1;cwd;/a b\x07",
        b"\x1b]2222;n1;cwd;%4\x07",
        b"\x1b]2222;n1;cwd;%zz\x07",
        b"\x1b]2222;n1;cwd;\xc3\xa9\x07",
        b"\x1b]2222;n1;aliases;a\xffb\x07",
        // Not ended by BEL: cut short.
        b"\x1b]2222;n1;ready\x1b\\",
        b"\x1b]2222;n1;ready\x18",
        b"\x1b]2222;n1;ready\x1b[H",
    ] {
        assert_eq!(reports(text), [], "{text:?}");
    }
    // Without integration nothing is a report.
    let mut core = core(10, 1, b"");
    assert_eq!(
        core.interpret(b"\x1b]2222;;ready\x07\x1b]2222;n1;ready\x07"),
        []
    );
}

/// A `functions` report of `count` names, each ten bytes long with the space after it.
fn functions(count: usize) -> Vec<u8> {
    let names: Vec<String> = (0..count).map(|name| format!("name{name:05}")).collect();
    [
        &b"\x1b]2222;n1;functions;"[..],
        names.join(" ").as_bytes(),
        b"\x07",
    ]
    .concat()
}

#[test]
fn a_long_report_arrives_whole() {
    // 200 KB, as a zsh with many plugins, or PowerShell with many modules, may send.
    let [Report::Functions(names)] = &reports(&functions(20_000))[..] else {
        panic!("one functions report");
    };
    assert_eq!(names.len(), 20_000);
    assert_eq!(names[19_999], "name19999");
}

#[test]
fn a_report_over_the_limit_is_dropped_and_what_follows_is_read() {
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    let text = [&functions(30_000)[..], b"after\x1b]2222;n1;ready\x07"].concat();
    assert!(text.len() > crate::REPORT_LIMIT);
    assert_eq!(core.interpret(&text), [Event::Report(Report::Ready)]);
    assert_eq!(rows(&core), ["after"]);
}

// Resizing

#[test]
fn resizing_cuts_and_pads_rows() {
    let mut core = core(10, 2, b"abcdefghij");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["abcde", ""]);
    assert_eq!(cursor(&core), (0, 4));
    core.resize(Size { cols: 8, rows: 3 });
    assert_eq!(rows(&core), ["abcde", "", ""]);
    assert_eq!(core.screen().rows()[0].cells().len(), 8);
}

#[test]
fn fewer_rows_go_from_the_top_only_to_keep_the_cursor() {
    let mut core = core(5, 4, &numbered(4));
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(cursor(&core), (1, 1));

    let mut core = self::core(5, 4, &numbered(4));
    core.interpret(b"\x1b[2H");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["1", "2"]);
    assert_eq!(cursor(&core), (1, 0));
}

#[test]
fn the_primary_screen_keeps_its_rows_while_the_alternate_is_shown() {
    let mut core = core(5, 4, &numbered(4));
    core.interpret(b"\x1b[?1049h\x1b[Hvim");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["vim", ""]);
    core.interpret(b"\x1b[?1049l");
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(cursor(&core), (1, 1));
}

#[test]
fn resizing_resets_the_region_and_extends_the_tab_stops() {
    let mut core = core(10, 4, &numbered(4));
    core.interpret(b"\x1b[1;2r");
    core.resize(Size { cols: 20, rows: 4 });
    core.interpret(b"\x1b[4H\n\t\tx");
    assert_eq!(rows(&core), ["2", "3", "4", "                x"]);
}

#[test]
fn a_size_of_zero_is_one() {
    let mut core = TerminalCore::new(Size { cols: 0, rows: 0 }, None);
    assert_eq!(core.size(), Size { cols: 1, rows: 1 });
    core.interpret(b"ab\x1b[5;5H\x1b[L\x1b[P\tc");
    assert_eq!(rows(&core), ["c"]);
    core.resize(Size { cols: 0, rows: 3 });
    assert_eq!(core.size(), Size { cols: 1, rows: 3 });
}

// Deterministic, whatever the input (ARC-03, REL-02)

/// xorshift64: the same numbers for the same seed, on every machine.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Pieces of control functions, characters and garbage, for building hostile output.
const PIECES: &[&[u8]] = &[
    b"\x1b",
    b"\x1b[",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"[",
    b"]",
    b"?",
    b">",
    b"=",
    b"$",
    b"!",
    b" ",
    b"#",
    b"(",
    b")",
    b";",
    b":",
    b"0",
    b"1",
    b"2",
    b"5",
    b"6",
    b"9",
    b"25",
    b"47",
    b"1049",
    b"1047",
    b"1048",
    b"2222",
    b"n1",
    b"65535",
    b"99999999",
    b"@",
    b"A",
    b"B",
    b"C",
    b"D",
    b"E",
    b"G",
    b"H",
    b"I",
    b"J",
    b"K",
    b"L",
    b"M",
    b"P",
    b"S",
    b"T",
    b"W",
    b"X",
    b"Z",
    b"b",
    b"c",
    b"d",
    b"g",
    b"h",
    b"l",
    b"m",
    b"n",
    b"p",
    b"q",
    b"r",
    b"s",
    b"t",
    b"u",
    b"7",
    b"8",
    b"\n",
    b"\r",
    b"\t",
    b"\x08",
    b"\x0e",
    b"\x0f",
    b"\x07",
    b"\x18",
    b"\x1a",
    b"\x05",
    b"\x7f",
    b"\x9b",
    b"\xc2",
    b"\xc2\x9b",
    b"\xe2\x94\x80",
    b"\xf0\x9f\x98\x80",
    b"\xff",
    b"\x1b\\",
    b"cwd;",
    b"ready",
    b"%",
    b"x",
    b"hello ",
];

/// `length` bytes or so of hostile output.
fn hostile(random: &mut Random, length: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(length + 8);
    while bytes.len() < length {
        if random.below(8) == 0 {
            bytes.push(random.next() as u8);
        } else {
            bytes.extend_from_slice(PIECES[random.below(PIECES.len())]);
        }
    }
    bytes
}

/// Output with a little of everything, to cut in pieces.
const SAMPLE: &[u8] =
    b"hello\r\n\x1b[31mred\x1b[m \xe2\x94\x80 caf\xc3\xa9 \xc2\x85\x1b[2;5r\x1b[5H\n\n\n\
                       \x1b]2222;n1;cwd;/tmp\x07\x1b[?1049hfull\x1b[6n\x1b[?1049l\x1b(0lqk\x1b(Bz\
                       \x1b[3b\tx\x1b[c\x1b]2222;n1;ready\x07\xd0\xb9 \xd0\xbc\xf0\x9f\x98\x80!";

/// The state and events from `text`, given in pieces `cut` long.
fn cut(text: &[u8], mut cut: impl FnMut() -> usize) -> (TerminalCore, Vec<Event>) {
    let mut core = TerminalCore::new(Size { cols: 20, rows: 6 }, Some("n1"));
    let mut events = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (piece, after) = rest.split_at(cut().min(rest.len()));
        events.extend(core.interpret(piece));
        rest = after;
    }
    (core, events)
}

#[test]
fn the_same_bytes_give_the_same_screen_wherever_they_are_cut() {
    let whole = cut(SAMPLE, || usize::MAX);
    for at in 1..SAMPLE.len() {
        let mut pieces = [at, usize::MAX].into_iter();
        let (core, events) = cut(SAMPLE, || pieces.next().unwrap());
        assert!(core.terminal == whole.0.terminal, "cut at {at}");
        assert_eq!(events, whole.1, "cut at {at}");
    }
}

#[test]
fn the_same_bytes_give_the_same_screen_however_they_are_split() {
    for seed in 1..=50_u64 {
        let mut random = Random(seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
        let text = [SAMPLE, &hostile(&mut random, 5_000)].concat();
        let whole = cut(&text, || usize::MAX);
        let bytewise = cut(&text, || 1);
        let pieces = cut(&text, || 1 + random.below(12));
        for (core, events) in [bytewise, pieces] {
            assert!(core.terminal == whole.0.terminal, "seed {seed}");
            assert_eq!(events, whole.1, "seed {seed}");
        }
    }
}

#[test]
fn a_character_cut_in_two_loses_nothing_around_it() {
    // vte 0.15 alone drops the byte after a character it finishes, when another follows.
    for (first, second, row) in [
        (&b"\xd0"[..], &b"\xb9 \xd0\xbc"[..], "\u{439} \u{43c}"),
        (b"ab\xc3", b"\xa9\r\xc3\xa9", "\u{e9}b\u{e9}"),
        (b"caf\xc3", b"\xa9 \xc2\x85x", "caf\u{e9} x"),
        (b"\xe2\x94", b"\x80\x08\xe2\x94\x82", "\u{2502}"),
        (
            b"\xf0\x9f",
            b"\x98\x80 \xf0\x9f\x98\x80",
            "\u{1f600} \u{1f600}",
        ),
        (b"\xc3", b"\xa9\x1b[2J\x1b[H\xc3\xa9", "\u{e9}"),
        (b"\xc3", b"x", "\u{fffd}x"),
    ] {
        let mut core = core(10, 2, first);
        core.interpret(second);
        assert_eq!(rows(&core)[0], row, "{first:?} {second:?}");
        assert!(core.terminal == self::core(10, 2, &[first, second].concat()).terminal);
    }
}

#[test]
fn resizing_to_the_same_size_changes_nothing() {
    let mut core = core(10, 3, b"\x1b[2;3r0123456789");
    core.resize(Size { cols: 10, rows: 3 });
    // The wrap is still pending, and the region is as the program set it.
    core.interpret(b"x\x1b[3H\n");
    assert_eq!(rows(&core), ["0123456789", "", ""]);
}

#[test]
fn hostile_output_never_breaks_the_rules() {
    let sizes = [(1, 1), (1, 2), (2, 1), (3, 5), (80, 24)];
    let mut random = Random(0x0016_0016);
    for (cols, rows) in sizes {
        let mut core = TerminalCore::new(Size { cols, rows }, Some("n1"));
        for _ in 0..2_000 {
            let length = 1 + random.below(64);
            core.interpret(&hostile(&mut random, length));
            core.terminal.check();
            if random.below(100) == 0 {
                let (cols, rows) = sizes[random.below(sizes.len())];
                core.resize(Size { cols, rows });
                core.terminal.check();
            }
        }
    }
}

#[test]
fn a_terminal_fits_on_a_small_stack() {
    // The parser is built on the stack before it is boxed.
    let size = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| TerminalCore::new(Size { cols: 80, rows: 24 }, None).size())
        .expect("a thread")
        .join()
        .expect("no overflow");
    assert_eq!(size, Size { cols: 80, rows: 24 });
}

/// PERF-06: plain text at 25 MB/s or more. Run with
/// `cargo test --release -p terminal-core -- --ignored`.
#[test]
#[ignore = "a measurement, for release builds"]
fn plain_text_is_read_at_25_mb_a_second() {
    let line = [&[b'x'; 78][..], b"\r\n"].concat();
    let chunk: Vec<u8> = line.iter().copied().cycle().take(64 * 1024).collect();
    let mut core = TerminalCore::new(Size { cols: 80, rows: 24 }, None);
    let total = 100 * 1024 * 1024;
    let start = std::time::Instant::now();
    for _ in 0..total / chunk.len() {
        core.interpret(&chunk);
    }
    let seconds = start.elapsed().as_secs_f64();
    let rate = total as f64 / seconds / 1_000_000.0;
    std::println!("{rate:.0} MB/s");
    assert!(rate >= 25.0, "{rate:.1} MB/s");
}
