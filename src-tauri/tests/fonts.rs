//! Bundled font test (SRS §4.7).
//!
//! Checks that the default font ships whole: JetBrains Mono's four faces, each named by the
//! page's style sheet with its weight and style, and the font's copyright notice and license,
//! which the installers hold too (FONT-01, C-3). DESIGN.md §7 says where they are.

use std::fs;
use std::path::{Path, PathBuf};

/// Each face's file in `src/fonts/`, and the weight and style `src/app.css` gives it.
const FACES: [(&str, &str, &str); 4] = [
    ("JetBrainsMono-Regular.woff2", "400", "normal"),
    ("JetBrainsMono-Bold.woff2", "700", "normal"),
    ("JetBrainsMono-Italic.woff2", "400", "italic"),
    ("JetBrainsMono-BoldItalic.woff2", "700", "italic"),
];

/// The font's copyright notice and license.
const LICENSE: &str = "src/fonts/OFL.txt";

#[test]
fn the_four_faces_are_fonts_tauri_serves_as_fonts() {
    for (file, ..) in FACES {
        let path = format!("src/fonts/{file}");
        let bytes = fs::read(root().join(&path)).unwrap_or_else(|error| panic!("{path}: {error}"));
        // Tauri tells a file's type by its first bytes, and takes WOFF2 for a font only when
        // its outlines are TrueType's. Anything else would be served as a page.
        assert_eq!(bytes.get(..4), Some(&b"wOF2"[..]), "{path} is not WOFF2");
        assert_eq!(
            bytes.get(4..8),
            Some(&[0, 1, 0, 0][..]),
            "{path} does not have TrueType outlines"
        );
    }
}

#[test]
fn the_style_sheet_names_each_face() {
    let css = read("src/app.css");
    let rules: Vec<&str> = css
        .split("@font-face")
        .skip(1)
        .map(|rule| &rule[..rule.find('}').expect("a rule ends")])
        .collect();
    assert_eq!(rules.len(), FACES.len(), "src/app.css names each face once");

    for (file, weight, style) in FACES {
        let source = format!("src: url(\"fonts/{file}\") format(\"woff2\");");
        let rule = rules
            .iter()
            .find(|rule| rule.contains(&source))
            .unwrap_or_else(|| panic!("src/app.css names no face {file}"));
        // The name hides an installed copy, so every machine draws the bundled one.
        for declaration in [
            "font-family: \"JetBrains Mono\";".to_string(),
            format!("font-weight: {weight};"),
            format!("font-style: {style};"),
        ] {
            assert!(rule.contains(&declaration), "{file} needs `{declaration}`");
        }
    }
}

#[test]
fn the_license_ships_with_every_copy() {
    let license = read(LICENSE);
    assert!(
        license.starts_with("Copyright 2020 The JetBrains Mono Project Authors"),
        "{LICENSE} must start with the font's copyright notice"
    );
    assert!(
        license.contains("SIL OPEN FONT LICENSE Version 1.1"),
        "{LICENSE} must hold the license"
    );

    // Embedded with the page's files, and a file of its own in every installer.
    let config: String = read("src-tauri/tauri.conf.json")
        .split_whitespace()
        .collect();
    assert!(
        config.contains("\"../src/fonts/OFL.txt\":\"fonts/OFL.txt\""),
        "tauri.conf.json must bundle {LICENSE} as fonts/OFL.txt"
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the app crate sits one level below the repository root")
        .to_path_buf()
}

fn read(file: &str) -> String {
    fs::read_to_string(root().join(file)).unwrap_or_else(|error| panic!("{file}: {error}"))
}
