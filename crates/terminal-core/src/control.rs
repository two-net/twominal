//! What each control function does (TERM-01): the [`Perform`] of the [`Terminal`].
//!
//! vte splits a program's output into characters, control characters and escape sequences.
//! Here each is matched with what xterm does for it. Whatever is not matched is ignored, as
//! ECMA-48 asks (REL-02).
//!
//! Some queries are answered. A reply is made of fixed text and of numbers, never of bytes
//! the program wrote, so no program can make Twominal type for it (SEC-05). For that reason
//! these are never answered: the window title and icon label reports (`CSI 20 t`, `CSI 21 t`),
//! the answerback to ENQ, and every device control string, DECRQSS and XTGETTCAP among them.
//!
//! Left to their own issues: the cursor's shape (#25), the bell, titles and the other operating
//! system commands (#27).

use alloc::format;

use vte::{Params, Perform};

use crate::report;
use crate::terminal::{Charset, Mode, Terminal};

/// The answer to DA1: a VT220 with ANSI color. fish waits for it at startup.
const PRIMARY_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";

/// The answer to DA2: a VT220, version 2500. Programs choose by the version, vim's
/// `handle_version_response` among them. vim before 9.0.1168, whose own entry for xterm has no
/// `XM`, asks for SGR mouse reports and drags from 277 (TERM-10); later ones ask anyway. From
/// 2500 vim keeps sending underline colors, which it stops between 95 and 2499 (TERM-05). From
/// 141 it asks for terminal capabilities with XTGETTCAP, which is never answered. Emacs reads
/// 2001 to 4000 as VTE, and so asks for nothing. 4000 to 4009 would be kitty to vim.
const SECONDARY_ATTRIBUTES: &[u8] = b"\x1b[>1;2500;0c";

/// The answer to DSR 5: no malfunction.
const STATUS_OK: &[u8] = b"\x1b[0n";

impl Perform for Terminal {
    fn print(&mut self, char: char) {
        // DEL and the C1 controls are not characters. vte gives C1 controls to `execute`,
        // unless their two bytes came in two pieces, so they are treated as there (ARC-03).
        if matches!(char, '\x7f' | '\u{80}'..='\u{9f}') {
            self.forget_last();
        } else {
            self.print_char(char);
        }
    }

    fn execute(&mut self, byte: u8) {
        self.forget_last();
        match byte {
            0x08 => self.backspace(),
            0x09 => self.tab_forward(1),
            0x0a..=0x0c => self.line_feed(),
            0x0d => self.carriage_return(),
            0x0e => self.shift(1),
            0x0f => self.shift(0),
            // vte has already ended any sequence that CAN or SUB cut short.
            0x1a => self.substitute(),
            // BEL is #27's. ENQ asks for an answerback, which would be typed (SEC-05). vte
            // reads escape sequences only in their 7-bit form, so no C1 control is acted on.
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.forget_last();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.forget_last();
        if let Some(report) = report::read(params, bell_terminated, self.nonce()) {
            self.report(report);
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.forget_last();
        if ignore {
            return;
        }
        match (intermediates, byte) {
            ([], b'7') => self.save_cursor(),
            ([], b'8') => self.restore_cursor(),
            ([], b'D') => self.index(),
            ([], b'E') => self.next_line(),
            ([], b'M') => self.reverse_index(),
            ([], b'H') => self.set_tab_stop(),
            ([], b'c') => self.reset(),
            ([], b'=') => self.set_keypad(true),
            ([], b'>') => self.set_keypad(false),
            ([], b'n') => self.shift(2),
            ([], b'o') => self.shift(3),
            ([b'#'], b'8') => self.align(),
            // DECID, the VT100's DA1.
            ([], b'Z') => self.reply(PRIMARY_ATTRIBUTES.to_vec()),
            ([which @ b'('..=b'+'], _) => {
                if let Some(charset) = Charset::named(byte) {
                    self.designate(usize::from(which - b'('), charset);
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || !intermediates.is_empty() || action != 'b' {
            self.forget_last();
        }
        // Too many parameters or intermediates: acting on some of them would be wrong.
        if ignore {
            return;
        }
        // Only SGR gives sub-parameters a meaning.
        if action != 'm' && params.iter().any(|param| param.len() > 1) {
            return;
        }
        let arg = |index| arg(params, index);
        let count = |index| arg(index).max(1);

        match (intermediates, action) {
            ([], '@') => self.insert_chars(count(0)),
            ([], 'A') => self.move_up(count(0)),
            ([], 'B' | 'e') => self.move_down(count(0)),
            ([], 'C' | 'a') => self.move_forward(count(0)),
            ([], 'D') => self.move_back(count(0)),
            ([], 'E') => {
                self.move_down(count(0));
                self.carriage_return();
            }
            ([], 'F') => {
                self.move_up(count(0));
                self.carriage_return();
            }
            ([], 'G' | '`') => self.set_col(count(0) - 1),
            ([], 'H' | 'f') => self.goto(count(0) - 1, count(1) - 1),
            ([], 'I') => self.tab_forward(count(0)),
            ([] | [b'?'], 'J') => self.erase_display(arg(0)),
            ([] | [b'?'], 'K') => self.erase_line(arg(0)),
            ([], 'L') => self.insert_lines(count(0)),
            ([], 'M') => self.delete_lines(count(0)),
            ([], 'P') => self.delete_chars(count(0)),
            ([], 'S') => self.scroll_up(count(0)),
            // With more parameters it is xterm's highlight mouse tracking.
            ([], 'T') if params.len() == 1 => self.scroll_down(count(0)),
            ([], 'X') => self.erase_chars(count(0)),
            ([], 'Z') => self.tab_backward(count(0)),
            ([], 'b') => self.repeat(count(0)),
            ([], 'c') if arg(0) == 0 => self.reply(PRIMARY_ATTRIBUTES.to_vec()),
            ([b'>'], 'c') if arg(0) == 0 => self.reply(SECONDARY_ATTRIBUTES.to_vec()),
            ([], 'd') => self.set_row(count(0) - 1),
            ([], 'g') => match arg(0) {
                0 => self.clear_tab_stop(),
                3 => self.clear_tab_stops(),
                _ => {}
            },
            ([], 'h' | 'l') => {
                for number in params.iter().map(number) {
                    if let Some(mode) = Mode::ansi(number) {
                        self.set_mode(mode, action == 'h');
                    }
                }
            }
            ([b'?'], 'h' | 'l') => {
                for number in params.iter().map(number) {
                    if let Some(mode) = Mode::dec(number) {
                        self.set_mode(mode, action == 'h');
                    }
                }
            }
            ([], 'm') => self.select_style(params.iter()),
            ([], 'n') => match arg(0) {
                5 => self.reply(STATUS_OK.to_vec()),
                6 => {
                    let (row, col) = self.position();
                    self.reply(format!("\x1b[{row};{col}R").into_bytes());
                }
                _ => {}
            },
            ([b'?'], 'n') if arg(0) == 6 => {
                let (row, col) = self.position();
                self.reply(format!("\x1b[?{row};{col}R").into_bytes());
            }
            ([], 'r') => self.set_region(arg(0), arg(1)),
            ([], 's') => self.save_cursor(),
            ([], 'u') => self.restore_cursor(),
            // The kitty keyboard protocol (TERM-09). vte reads a missing parameter as 0, so
            // `CSI < 0 u` pops one, as `CSI < u` does, where kitty pops none.
            ([b'>'], 'u') => self.push_keyboard(arg(0)),
            ([b'<'], 'u') => self.pop_keyboard(count(0)),
            ([b'='], 'u') => {
                // A missing mode is 1, and an empty one 0, which does nothing, as in kitty.
                let how = if params.len() < 2 { 1 } else { arg(1) };
                self.set_keyboard(arg(0), how);
            }
            // Whatever its parameters, as kitty answers it.
            ([b'?'], 'u') => {
                let flags = self.modes().keyboard.bits();
                self.reply(format!("\x1b[?{flags}u").into_bytes());
            }
            ([b'?'], 'W') if arg(0) == 5 => self.reset_tab_stops(),
            ([b'$'], 'p') => {
                let number = first(params);
                let state = self.state(Mode::ansi(number));
                self.reply(format!("\x1b[{number};{state}$y").into_bytes());
            }
            ([b'?', b'$'], 'p') => {
                let number = first(params);
                let state = self.state(Mode::dec(number));
                self.reply(format!("\x1b[?{number};{state}$y").into_bytes());
            }
            ([b'!'], 'p') => self.soft_reset(),
            _ => {}
        }
    }
}

impl Terminal {
    /// How DECRQM reports `mode`: 1 set, 2 reset, 0 not known.
    fn state(&self, mode: Option<Mode>) -> u8 {
        match mode.and_then(|mode| self.mode(mode)) {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        }
    }
}

/// The parameter at `index`, 0 when it is missing or empty.
fn arg(params: &Params, index: usize) -> usize {
    params
        .iter()
        .nth(index)
        .map_or(0, |param| usize::from(number(param)))
}

/// The first parameter.
fn first(params: &Params) -> u16 {
    params.iter().next().map_or(0, number)
}

/// A parameter's value, without sub-parameters.
fn number(param: &[u16]) -> u16 {
    param.first().copied().unwrap_or(0)
}
