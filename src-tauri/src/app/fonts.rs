//! The font the pages draw with, and whether it is installed. Part of the App Shell.
//!
//! The Font and Font size settings become a CSS family list and a size in CSS pixels, which
//! each page hands to its Renderer (FONT-02, SW-06). The list ends in the bundled font,
//! JetBrains Mono, whose faces `src/app.css` names from `src/fonts/` (FONT-01), so a font that
//! is not installed is drawn as the default. Whether it is installed is found out here, from
//! the OS's font folders, so that it gets a notice (SET-05). The zoom items step the size.
//! DESIGN.md §3 has the rules.

use std::fmt::Write;
use std::str;

use ttf_parser::{name_id, PlatformId};

use crate::settings::{Settings, FONT_SIZES};

/// The bundled font's family, as `src/app.css` names its faces.
pub(super) const BUNDLED: &str = "JetBrains Mono";

/// How many CSS pixels a point is on Linux and Windows, which count 96 pixels to the inch.
/// macOS counts 72, so there a point is a pixel.
const PIXELS_PER_POINT: f64 = 96.0 / 72.0;

/// What a font size item asks for (FONT-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Zoom {
    Bigger,
    Smaller,
    Reset,
}

/// The font a page draws with: a CSS family list, and its size in CSS pixels. `macos` says
/// which OS's points the Font size setting is in.
pub(super) fn font(settings: &Settings, macos: bool) -> (String, f64) {
    let chosen = settings.font.trim();
    let mut families = Vec::new();
    if !is_bundled(chosen) {
        families.push(css_string(chosen));
    }
    // A character no font before it has comes from the OS's monospace font, and then from
    // whatever the OS falls back to (FONT-11, SW-06).
    families.extend([css_string(BUNDLED), "monospace".to_string()]);
    let size = if macos {
        settings.font_size
    } else {
        settings.font_size * PIXELS_PER_POINT
    };
    (families.join(", "), size)
}

/// Whether `font` names the bundled font. CSS matches a family name whatever its letter case.
pub(super) fn is_bundled(font: &str) -> bool {
    font.trim().eq_ignore_ascii_case(BUNDLED)
}

/// `text` as a CSS string. A canvas ignores a `font` it cannot read, so every quote and
/// backslash is escaped, and every control character written as its code.
fn css_string(text: &str) -> String {
    let mut css = String::with_capacity(text.len() + 2);
    css.push('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                css.push('\\');
                css.push(c);
            }
            // The space ends the code, so the character after it cannot be read as more of it.
            c if c.is_control() => {
                let _ = write!(css, "\\{:x} ", u32::from(c));
            }
            c => css.push(c),
        }
    }
    css.push('"');
    css
}

/// The font size `zoom` makes of `size`, in points: the next whole point up or down, as far
/// as the setting goes, or the default.
pub(super) fn zoomed(size: f64, zoom: Zoom) -> f64 {
    match zoom {
        Zoom::Bigger => (size.floor() + 1.0).min(*FONT_SIZES.end()),
        Zoom::Smaller => (size.ceil() - 1.0).max(*FONT_SIZES.start()),
        Zoom::Reset => Settings::default().font_size,
    }
}

/// Whether a font family named `font` is installed, letter case aside. The bundled font always
/// is. Every other name has the OS's font folders read, which takes a while, so it is asked
/// from a thread of its own.
pub(super) fn installed(font: &str) -> bool {
    let font = font.trim();
    if is_bundled(font) {
        return true;
    }
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let found = database.faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(font))
            || database
                .with_face_data(face.id, |data, index| has_family(data, index, font))
                .unwrap_or(false)
    });
    found
}

/// Whether face `index` of the font file `data` has the family name `font`, letter case aside.
/// fontdb keeps one family name of each face, but a webview matches a CSS family with any of
/// three: name IDs 1, 16 and 21, in every language.
pub(super) fn has_family(data: &[u8], index: u32, font: &str) -> bool {
    let names = ttf_parser::RawFace::parse(data, index)
        .ok()
        .and_then(|face| face.table(ttf_parser::Tag::from_bytes(b"name")))
        .and_then(ttf_parser::name::Table::parse);
    let Some(names) = names else {
        return false;
    };
    names
        .names
        .into_iter()
        .filter(|name| {
            matches!(
                name.name_id,
                name_id::FAMILY | name_id::TYPOGRAPHIC_FAMILY | name_id::WWS_FAMILY
            )
        })
        .filter_map(|name| match name.platform_id {
            // Older Mac fonts, such as Menlo, have their names in Mac Roman only. Its first
            // half is ASCII, and fontdb reads the rest.
            PlatformId::Macintosh if name.encoding_id == 0 => str::from_utf8(name.name)
                .ok()
                .filter(|name| name.is_ascii())
                .map(String::from),
            _ => name.to_string(),
        })
        .any(|family| family.eq_ignore_ascii_case(font))
}
