//! What shell integration reports (FISH-05), read back from `ESC ] 2222 ; nonce ; key ; value
//! BEL`. DESIGN.md §7 has the format and the keys.
//!
//! Only a report that carries the session's nonce counts, so text that is merely printed, such
//! as a file shown with `cat`, cannot fake one. Like any other operating system command, a
//! report never shows on the screen, whether it counts or not.

use alloc::string::String;
use alloc::vec::Vec;

use crate::REPORT_LIMIT;

/// What the shell integration reported (FISH-05).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The working directory (`cwd`). Bytes, because a path need not be UTF-8.
    Directory(Vec<u8>),
    /// The command search path (`path`), as the shell has it.
    SearchPath(Vec<u8>),
    /// The names of the shell's aliases (`aliases`).
    Aliases(Vec<String>),
    /// The names of the shell's functions (`functions`).
    Functions(Vec<String>),
    /// The names of PowerShell's cmdlets, which count as builtins (`cmdlets`; CMD-07, CMD-09).
    Cmdlets(Vec<String>),
    /// The shell is ready for a command (`ready`).
    Ready,
}

/// The report an operating system command holds, if it is one that counts: OSC 2222, carrying
/// `nonce`, written as the scripts write it, and ended by BEL.
pub(crate) fn read(
    params: &[&[u8]],
    bell_terminated: bool,
    nonce: Option<&[u8]>,
) -> Option<Report> {
    let nonce = nonce?;
    // Ended by anything but BEL, it was cut short: by an ESC, a CAN or a SUB.
    if !bell_terminated {
        return None;
    }
    // vte keeps no more than this, so one that long was cut short too.
    let length =
        params.iter().map(|param| param.len()).sum::<usize>() + params.len().saturating_sub(1);
    if length >= REPORT_LIMIT {
        return None;
    }
    match params {
        [b"2222", given, b"ready"] if *given == nonce => Some(Report::Ready),
        [b"2222", given, key, value] if *given == nonce => Some(match *key {
            b"cwd" => Report::Directory(decode(value)?),
            b"path" => Report::SearchPath(decode(value)?),
            b"aliases" => Report::Aliases(names(value)?),
            b"functions" => Report::Functions(names(value)?),
            b"cmdlets" => Report::Cmdlets(names(value)?),
            _ => return None,
        }),
        _ => None,
    }
}

/// `value` with each `%XX` read back as its byte. `None` if it holds a byte outside `!` to
/// `~`, or a `%` without two hex digits after it: the scripts never write that.
fn decode(value: &[u8]) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut rest = value;
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        match byte {
            b'%' => {
                let [high, low, after @ ..] = rest else {
                    return None;
                };
                bytes.push(hex(*high)? << 4 | hex(*low)?);
                rest = after;
            }
            b'!'..=b'~' => bytes.push(byte),
            _ => return None,
        }
    }
    Some(bytes)
}

/// The names in a list, which single spaces separate. A name that is not UTF-8 is left out:
/// nobody could type it.
fn names(value: &[u8]) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for name in value.split(|&byte| byte == b' ') {
        if let Ok(name) = String::from_utf8(decode(name)?) {
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    Some(names)
}

fn hex(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|value| value as u8)
}
