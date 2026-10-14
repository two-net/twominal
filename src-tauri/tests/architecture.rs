//! Architecture test (SRS §6.3).
//!
//! Checks the code against the two marked tables in `DESIGN.md`: the components with their
//! homes (§1) and the direct dependencies (§2). To change the structure, change the document
//! first. DESIGN.md §10 says which requirement each test answers.
//!
//! Cargo already enforces the direction between the tiers, because each lower tier is its own
//! crate. This test covers what Cargo cannot see.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The components of SRS §6.1, and the Tab that holds the per-tab ones. Changing this list
/// means changing the SRS (ARC-01).
const SRS_COMPONENTS: [&str; 12] = [
    "App Shell",
    "Settings Store",
    "History Store",
    "Theme Manager",
    "Animator",
    "Tab Manager",
    "Tab",
    "Session",
    "Terminal Core",
    "Command Line",
    "Input Handler",
    "Renderer",
];

/// Where the webview's files are.
const WEBVIEW: &str = "src";

// ---------------------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------------------

#[test]
fn every_component_has_its_home() {
    let components = components();
    let mut problems = Vec::new();

    let listed: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
    if listed != SRS_COMPONENTS {
        problems.push(format!(
            "DESIGN.md §1 lists {listed:?}, but SRS §6.1 has {SRS_COMPONENTS:?}"
        ));
    }

    let mut seen = BTreeSet::new();
    for component in &components {
        if component.rust.is_none() && component.webview.is_none() {
            problems.push(format!("{} has no home", component.name));
        }
        if let Some(home) = &component.rust {
            if !root().join(home).is_file() {
                problems.push(format!("{}: {home} does not exist", component.name));
            }
        }
        for home in component.rust.iter().chain(&component.webview) {
            if !seen.insert(home.clone()) {
                problems.push(format!("{home} is the home of two components"));
            }
        }
    }

    assert_none(problems, "DESIGN.md §1 and the code disagree");
}

#[test]
fn no_code_outside_a_component() {
    let components = components();
    let mut problems = Vec::new();

    for dir in crate_dirs(&components) {
        for file in walk(&dir, &["rs"]) {
            if rust_owner(&file, &components).is_none() && !is_crate_root(&file) {
                problems.push(format!("{file} belongs to no component"));
            }
        }
    }
    for file in walk(WEBVIEW, &["js"]) {
        if webview_owner(&file, &components).is_none() {
            problems.push(format!("{file} belongs to no component"));
        }
    }

    assert_none(
        problems,
        "give each file a home in DESIGN.md §1, or move it under its component",
    );
}

#[test]
fn components_do_not_name_their_siblings() {
    let components = components();
    let mut problems = Vec::new();

    for dir in crate_dirs(&components) {
        let modules = modules_in(&dir, &components);
        for file in walk(&dir, &["rs"]) {
            let Some(owner) = rust_owner(&file, &components) else {
                continue;
            };
            // The two places where wiring happens: the App Shell, and a crate's root.
            if owner.tier == 1 || is_crate_root(&file) {
                continue;
            }
            let own = module_of(owner.rust.as_deref().unwrap());
            let text = strip(&read(&file), Language::Rust);
            let mut named = paths_after(&text, "crate::");
            if is_module_root(&file, &dir) {
                named.extend(paths_after(&text, "super::"));
            }
            for name in named {
                if name == "*" {
                    problems.push(format!("{file} imports everything from the crate root"));
                } else if name != own && modules.contains(&name) {
                    problems.push(format!("{file} ({}) names `{name}`", owner.name));
                }
            }
        }
    }

    for file in walk(WEBVIEW, &["js"]) {
        let Some(owner) = webview_owner(&file, &components) else {
            continue;
        };
        if owner.tier == 1 {
            continue;
        }
        for specifier in js_imports(&strip(&read(&file), Language::JavaScript)) {
            let Some(target) = resolve(&file, &specifier) else {
                continue;
            };
            if let Some(other) = webview_owner(&target, &components) {
                if other.name != owner.name {
                    problems.push(format!(
                        "{file} ({}) imports {target} ({})",
                        owner.name, other.name
                    ));
                }
            }
        }
    }

    assert_none(
        problems,
        "a component names only lower tiers, never a sibling (ARC-02, DESIGN.md §1)",
    );
}

#[test]
fn only_the_app_shell_names_tauri() {
    let components = components();
    let mut problems = Vec::new();

    for dir in crate_dirs(&components) {
        for file in walk(&dir, &["rs"]) {
            let shell = rust_owner(&file, &components).is_some_and(|c| c.tier == 1);
            if !shell && has_word(&strip(&read(&file), Language::Rust), "tauri") {
                problems.push(format!("{file} names `tauri`"));
            }
        }
    }
    for file in walk(WEBVIEW, &["js", "html"]) {
        let shell = webview_owner(&file, &components).is_some_and(|c| c.tier == 1);
        if !shell && read(&file).contains("__TAURI__") {
            problems.push(format!("{file} touches `window.__TAURI__`"));
        }
    }

    assert_none(
        problems,
        "only the App Shell talks to Tauri; hand the others what they need (ARC-02, DESIGN.md §1)",
    );
}

#[test]
fn terminal_core_is_no_std() {
    let components = components();
    let core = components
        .iter()
        .find(|c| c.name == "Terminal Core")
        .and_then(|c| c.rust.clone())
        .expect("the Terminal Core has a Rust home");
    let mut problems = Vec::new();

    if !strip(&read(&core), Language::Rust).contains("#![no_std]") {
        problems.push(format!("{core} is not `#![no_std]`"));
    }
    let dir = core.rsplit_once('/').unwrap().0;
    for file in walk(dir, &["rs"]) {
        let text = strip(&read(&file), Language::Rust);
        let mut previous = "";
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let test_only = line.contains("cfg(test)") || previous.contains("cfg(test)");
            if line.contains("extern crate std") && !test_only {
                problems.push(format!("{file} brings `std` back in"));
            }
            previous = line;
        }
    }

    assert_none(
        problems,
        "the Terminal Core must not be able to reach the OS (ARC-02, ARC-03)",
    );
}

#[test]
fn dependencies_match_the_design() {
    let designed: BTreeSet<(String, String)> = table(&read("DESIGN.md"), "dependencies")
        .into_iter()
        .map(|row| (row[0].clone(), row[1].clone()))
        .collect();

    // Every package, not only the components' crates: the fuzz crate is outside the
    // workspace, but its dependencies are as direct as any.
    let mut actual = BTreeSet::new();
    for file in ["crates", "src-tauri"]
        .into_iter()
        .flat_map(|dir| walk(dir, &["toml"]))
        .filter(|file| file.ends_with("/Cargo.toml"))
    {
        let manifest = read(&file);
        let Some(package) = package_name(&manifest) else {
            continue;
        };
        for dependency in manifest_dependencies(&manifest) {
            actual.insert((package.clone(), dependency));
        }
    }

    let mut problems = Vec::new();
    for (package, dependency) in actual.difference(&designed) {
        problems.push(format!(
            "`{package}` depends on `{dependency}`, which DESIGN.md §2 does not list"
        ));
    }
    for (package, dependency) in designed.difference(&actual) {
        problems.push(format!(
            "DESIGN.md §2 lists `{dependency}` for `{package}`, which does not depend on it"
        ));
    }

    assert_none(
        problems,
        "add the row to DESIGN.md §2 first, then the dependency (ARC-06)",
    );
}

#[test]
fn webview_reaches_nothing_but_ipc() {
    let config = read("src-tauri/tauri.conf.json");
    let Some(policy) = json_string(&config, "csp") else {
        panic!("src-tauri/tauri.conf.json sets no content security policy (ARC-06)");
    };
    let directives = csp_directives(&policy);
    let mut problems = Vec::new();

    let sources = |name: &str| {
        directives
            .iter()
            .find(|(directive, _)| directive == name)
            .map(|(_, sources)| sources.clone())
    };
    if sources("default-src") != Some(vec!["'self'".to_string()]) {
        problems.push("`default-src` must be exactly 'self'".to_string());
    }
    if sources("connect-src").is_none() {
        problems.push("`connect-src` must be set".to_string());
    }
    for (directive, sources) in &directives {
        for source in sources {
            if is_network_source(source) {
                problems.push(format!("`{directive}` allows {source}"));
            }
        }
    }

    assert_none(
        problems,
        "the webview may load the app's own files and talk to Rust, nothing else (ARC-06)",
    );
}

#[test]
fn animation_policy_lives_in_the_animator() {
    let components = components();
    let mut problems = Vec::new();

    for file in walk(WEBVIEW, &["js", "css", "html"]) {
        let animator = webview_owner(&file, &components).is_some_and(|c| c.name == "Animator");
        let text = read(&file);
        if !animator && text.contains("prefers-reduced-motion") {
            problems.push(format!("{file} reads the reduced-motion setting"));
        }
        if file.ends_with(".css") && css_animates(&strip(&text, Language::Css)) {
            problems.push(format!("{file} declares a transition or an animation"));
        }
    }

    assert_none(
        problems,
        "every interface animation is run by the Animator (ARC-05, DESIGN.md §5)",
    );
}

// ---------------------------------------------------------------------------------------
// DESIGN.md
// ---------------------------------------------------------------------------------------

struct Component {
    name: String,
    tier: u8,
    rust: Option<String>,
    webview: Option<String>,
}

fn components() -> Vec<Component> {
    let home = |cell: &str| (cell != "—").then(|| cell.to_string());
    table(&read("DESIGN.md"), "components")
        .into_iter()
        .map(|row| Component {
            name: row[0].clone(),
            tier: row[1].parse().expect("a tier is a number"),
            rust: home(&row[2]),
            webview: home(&row[3]),
        })
        .collect()
}

/// The rows of the table between `<!-- name:begin -->` and `<!-- name:end -->`, without the
/// header, as cells with their backticks removed.
fn table(document: &str, name: &str) -> Vec<Vec<String>> {
    let begin = format!("<!-- {name}:begin -->");
    let end = format!("<!-- {name}:end -->");
    let start = document
        .find(&begin)
        .unwrap_or_else(|| panic!("DESIGN.md has no `{begin}` marker"));
    let length = document[start..]
        .find(&end)
        .unwrap_or_else(|| panic!("DESIGN.md has no `{end}` marker"));

    document[start + begin.len()..start + length]
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().trim_matches('`').to_string())
                .collect::<Vec<_>>()
        })
        .filter(|cells| !cells.iter().all(|cell| cell.chars().all(|c| c == '-')))
        .skip(1)
        .collect()
}

// ---------------------------------------------------------------------------------------
// Files and who owns them
// ---------------------------------------------------------------------------------------

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the app crate sits one level below the repository root")
        .to_path_buf()
}

fn read(file: &str) -> String {
    fs::read_to_string(root().join(file)).unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// Every file under `dir` with one of the extensions, as sorted repository paths. Cargo's
/// `target` folders are not looked in.
fn walk(dir: &str, extensions: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_string()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(root().join(&dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = format!("{dir}/{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                if entry.file_name() != "target" {
                    pending.push(path);
                }
            } else if extensions
                .iter()
                .any(|extension| path.ends_with(&format!(".{extension}")))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The `src` folder of each crate that holds a component.
fn crate_dirs(components: &[Component]) -> BTreeSet<String> {
    components
        .iter()
        .filter_map(|c| c.rust.as_deref())
        .map(|home| {
            let end = home.find("/src/").expect("a Rust home is under src/") + "/src".len();
            home[..end].to_string()
        })
        .collect()
}

fn is_crate_root(file: &str) -> bool {
    file.ends_with("/src/lib.rs") || file.ends_with("/src/main.rs")
}

/// Whether `file` is the top file of a module directly under the crate root, so that
/// `super::` in it means the crate root.
fn is_module_root(file: &str, crate_dir: &str) -> bool {
    let inside = &file[crate_dir.len() + 1..];
    !inside.contains('/') || (inside.matches('/').count() == 1 && inside.ends_with("/mod.rs"))
}

/// `settings` for `src-tauri/src/settings.rs`.
fn module_of(home: &str) -> String {
    let file = home.rsplit_once('/').map_or(home, |(_, file)| file);
    file.rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .to_string()
}

/// The component modules of one crate. A crate root is not a module.
fn modules_in(crate_dir: &str, components: &[Component]) -> BTreeSet<String> {
    components
        .iter()
        .filter_map(|c| c.rust.as_deref())
        .filter(|home| home.starts_with(&format!("{crate_dir}/")) && !is_crate_root(home))
        .map(module_of)
        .collect()
}

/// A file belongs to a component if it is the component's home or lies in the folder named
/// after the home. A crate with a single home belongs to that component entirely.
fn rust_owner<'a>(file: &str, components: &'a [Component]) -> Option<&'a Component> {
    let homes = |dir: &str| {
        components
            .iter()
            .filter_map(|c| c.rust.as_deref())
            .filter(|home| home.starts_with(&format!("{dir}/")))
            .count()
    };
    components.iter().find(|component| {
        let Some(home) = component.rust.as_deref() else {
            return false;
        };
        let (dir, _) = home.rsplit_once('/').unwrap();
        file == home
            || file.starts_with(&format!("{}/", home.trim_end_matches(".rs")))
            || (is_crate_root(home) && homes(dir) == 1 && file.starts_with(&format!("{dir}/")))
    })
}

fn webview_owner<'a>(file: &str, components: &'a [Component]) -> Option<&'a Component> {
    components.iter().find(|component| {
        component.webview.as_deref().is_some_and(|home| {
            file == home || file.starts_with(&format!("{}/", home.trim_end_matches(".js")))
        })
    })
}

// ---------------------------------------------------------------------------------------
// Reading source text
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Language {
    Rust,
    JavaScript,
    Css,
}

/// Source text without its comments. Rust also loses the contents of its string literals,
/// so that nothing written in a string counts as code. JavaScript keeps them, because an
/// import names its target in one.
fn strip(source: &str, language: Language) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        if c == '/' && next == Some('/') && language != Language::Css {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            // Rust's block comments nest. The others end at the first `*/`.
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else if language == Language::Rust
                    && chars[i] == '/'
                    && chars.get(i + 1) == Some(&'*')
                {
                    depth += 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if language == Language::Rust && c == '\'' {
            // A character literal such as '"' or '\n'. A lifetime has no closing quote.
            let length = match (next, chars.get(i + 2)) {
                (Some('\\'), _) => chars
                    .get(i + 3..)
                    .and_then(|rest| rest.iter().position(|&c| c == '\''))
                    .map(|end| end + 4),
                (Some(_), Some('\'')) => Some(3),
                _ => None,
            };
            match length {
                Some(length) => {
                    out.push_str("' '");
                    i += length;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            }
        } else if c == '"' || (language != Language::Rust && (c == '\'' || c == '`')) {
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i] != c {
                let escaped = chars[i] == '\\' && i + 1 < chars.len();
                for _ in 0..if escaped { 2 } else { 1 } {
                    if language != Language::Rust {
                        out.push(chars[i]);
                    }
                    i += 1;
                }
            }
            if i < chars.len() {
                out.push(c);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn is_identifier(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c| !is_identifier(c))
        .filter(|word| !word.is_empty())
}

fn has_word(text: &str, word: &str) -> bool {
    identifiers(text).any(|identifier| identifier == word)
}

/// What follows each `prefix` (`crate::` or `super::`) in Rust source: the next path segment,
/// or the first segment of every item in a `{…}` group. `*` stands for a glob.
fn paths_after(text: &str, prefix: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(prefix) {
        let start = from + found;
        let rest = &text[start + prefix.len()..];
        from = start + prefix.len();

        // `mysuper::x` is not `super::x`.
        if text[..start].chars().next_back().is_some_and(is_identifier) {
            continue;
        }
        let rest = rest.trim_start();
        if let Some(group) = rest.strip_prefix('{') {
            names.extend(
                group_items(group)
                    .iter()
                    .filter_map(|item| first_segment(item)),
            );
        } else {
            names.extend(first_segment(rest));
        }
    }
    names
}

fn first_segment(path: &str) -> Option<String> {
    let path = path.trim_start();
    if path.starts_with('*') {
        return Some("*".to_string());
    }
    let end = path.find(|c| !is_identifier(c)).unwrap_or(path.len());
    (end > 0).then(|| path[..end].to_string())
}

/// The top-level items of a `use` group, given the text after its opening brace.
fn group_items(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => {
                items.push(&text[start..i]);
                return items;
            }
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items
}

/// The module specifiers a JavaScript file imports, statically or dynamically.
fn js_imports(text: &str) -> Vec<String> {
    let mut specifiers = Vec::new();
    let mut i = 0;
    while let Some(found) = text[i..].find(['"', '\'', '`']) {
        let open = i + found;
        let quote = text[open..].chars().next().unwrap();
        let Some(length) = text[open + 1..].find(quote) else {
            break;
        };
        let before = text[..open].trim_end().trim_end_matches('(').trim_end();
        let keyword = before
            .rsplit(|c| !is_identifier(c))
            .next()
            .unwrap_or_default();
        if keyword == "from" || keyword == "import" {
            specifiers.push(text[open + 1..open + 1 + length].to_string());
        }
        i = open + 1 + length + 1;
    }
    specifiers
}

/// The repository path a relative import points at.
fn resolve(importer: &str, specifier: &str) -> Option<String> {
    let base = if specifier.starts_with('/') {
        WEBVIEW
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        importer.rsplit_once('/')?.0
    } else {
        return None;
    };
    let mut parts: Vec<&str> = base.split('/').collect();
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Whether a stylesheet declares a transition, an animation or keyframes.
fn css_animates(text: &str) -> bool {
    if text.contains("@keyframes") {
        return true;
    }
    // A property starts a declaration: it follows `{` or `;`, and a colon follows it.
    ["transition", "animation"].iter().any(|property| {
        text.match_indices(property).any(|(start, _)| {
            let before = text[..start].trim_end().chars().next_back();
            let after = text[start + property.len()..]
                .trim_start_matches(|c: char| c.is_ascii_lowercase() || c == '-')
                .trim_start();
            before.is_none_or(|c| c == '{' || c == ';') && after.starts_with(':')
        })
    })
}

// ---------------------------------------------------------------------------------------
// Manifests and configuration
// ---------------------------------------------------------------------------------------

fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(|line| toml_code(line).trim()) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line.strip_prefix("name") {
                let value = value.trim_start().strip_prefix('=')?;
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Every direct dependency a manifest declares, in any table: normal, dev, build or
/// per-target, written inline or as its own table.
fn manifest_dependencies(manifest: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut in_dependencies = false;
    let mut depth = 0i32;

    for line in manifest.lines() {
        let line = toml_code(line);
        let line = line.trim();
        if depth == 0 && line.starts_with('[') {
            let header = line.trim_matches(|c| c == '[' || c == ']').trim();
            in_dependencies = header.ends_with("dependencies");
            if let Some((table, name)) = header.rsplit_once('.') {
                if table.ends_with("dependencies") {
                    found.insert(name.trim_matches(|c| c == '"' || c == '\'').to_string());
                }
            }
            continue;
        }
        if in_dependencies && depth == 0 {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().split('.').next().unwrap_or_default();
                let renamed = value
                    .split_once("package")
                    .and_then(|(_, rest)| rest.split('"').nth(1));
                found.insert(renamed.unwrap_or(key.trim_matches('"')).to_string());
            }
        }
        depth += line.matches(['{', '[']).count() as i32;
        depth -= line.matches(['}', ']']).count() as i32;
    }
    found
}

/// A TOML line without its comment.
fn toml_code(line: &str) -> &str {
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

/// The value of a string field in JSON text, or `None` if the field is absent or not a string.
fn json_string(json: &str, key: &str) -> Option<String> {
    let after = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let value = after.trim_start().strip_prefix(':')?.trim_start();
    let value = value.strip_prefix('"')?;
    Some(value[..value.find('"')?].to_string())
}

/// A content security policy as directives, each with its sources.
fn csp_directives(policy: &str) -> Vec<(String, Vec<String>)> {
    policy
        .split(';')
        .filter_map(|directive| {
            let mut words = directive.split_whitespace().map(str::to_string);
            Some((words.next()?, words.collect()))
        })
        .collect()
}

/// Whether a source lets the webview reach beyond the app's own files and its IPC with Rust.
/// `http://ipc.localhost` is how the IPC scheme is spelled on Windows.
fn is_network_source(source: &str) -> bool {
    let scheme =
        |name: &str| source == format!("{name}:") || source.starts_with(&format!("{name}://"));
    source != "http://ipc.localhost"
        && (source.contains('*')
            || ["http", "https", "ws", "wss", "ftp"]
                .into_iter()
                .any(scheme))
}

fn assert_none(problems: Vec<String>, rule: &str) {
    assert!(
        problems.is_empty(),
        "\n{rule}:\n  - {}\n",
        problems.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------------------
// The checks check themselves
// ---------------------------------------------------------------------------------------

#[test]
fn strip_drops_comments_and_rust_strings() {
    let rust = "use a; // crate::theme\n/* crate::tabs /* nested */ still */ let s = \"crate::app // x\"; let c = '\"'; fn f<'a>() {}";
    let stripped = strip(rust, Language::Rust);
    assert!(paths_after(&stripped, "crate::").is_empty(), "{stripped}");
    assert!(stripped.contains("fn f<'a>() {}"), "{stripped}");

    let js = "import a from \"./a.js\"; // import b from './b.js'\n/* import c from './c.js' */";
    assert_eq!(js_imports(&strip(js, Language::JavaScript)), ["./a.js"]);

    let css = "a { color: red; /* transition: none */ background: url(//x) }";
    assert!(!css_animates(&strip(css, Language::Css)));
}

#[test]
fn paths_after_sees_every_way_to_name_a_module() {
    let text = "use crate::theme::Palette;\n\
                use crate::{tabs, settings::{Settings, Theme}, history as h};\n\
                fn f() { crate::app::run(); super::tabs::open(); }\n\
                pub(crate) fn g() {}\n\
                use crate::*;\n\
                use mysuper::nothing;";
    assert_eq!(
        paths_after(text, "crate::"),
        ["theme", "tabs", "settings", "history", "app", "*"]
    );
    assert_eq!(paths_after(text, "super::"), ["tabs"]);
}

#[test]
fn js_imports_sees_every_way_to_import() {
    let text = "import { a } from './a.js';\n\
                import \"./b.js\";\n\
                export * from \"../c.js\";\n\
                const d = await import('/tab/d.js');\n\
                const text = 'from here';";
    assert_eq!(
        js_imports(text),
        ["./a.js", "./b.js", "../c.js", "/tab/d.js"]
    );
    assert_eq!(
        resolve("src/tab/input.js", "../animator.js").as_deref(),
        Some("src/animator.js")
    );
    assert_eq!(
        resolve("src/tabs.js", "/tab/d.js").as_deref(),
        Some("src/tab/d.js")
    );
    assert_eq!(resolve("src/tabs.js", "left-pad"), None);
}

#[test]
fn css_animates_sees_properties_not_words() {
    assert!(css_animates(".tab { transition: opacity 150ms }"));
    assert!(css_animates(".tab { color: red; animation-name: slide }"));
    assert!(css_animates("@keyframes slide { from { left: 0 } }"));
    assert!(!css_animates(
        ".tab { --tab-transition: 1; content: 'animation' }"
    ));
    assert!(!css_animates(
        ".animation-off, a.transition:hover { opacity: 1 }"
    ));
}

#[test]
fn manifest_dependencies_sees_every_table() {
    let manifest = r#"
        [package]
        name = "example" # not a dependency

        [dependencies]
        plain = "1"
        inline = { version = "1", features = [
            "a",
            "b",
        ] }
        dotted.workspace = true
        alias = { package = "real-name", version = "1" }

        [dev-dependencies]
        dev = "1"

        [build-dependencies]
        build = "1"

        [target.'cfg(windows)'.dependencies]
        windows-only = "1"

        [dependencies.own-table]
        version = "1"

        [features]
        default = ["plain"]
    "#;
    let expected = [
        "build",
        "dev",
        "dotted",
        "inline",
        "own-table",
        "plain",
        "real-name",
        "windows-only",
    ];
    assert_eq!(package_name(manifest).as_deref(), Some("example"));
    assert_eq!(
        manifest_dependencies(manifest),
        expected.into_iter().map(str::to_string).collect()
    );
}

#[test]
fn csp_checks_see_network_sources() {
    let json = r#"{ "security": { "devCsp": null, "csp": "default-src 'self'; connect-src ipc: http://ipc.localhost" } }"#;
    let policy = json_string(json, "csp").unwrap();
    assert_eq!(
        csp_directives(&policy),
        [
            ("default-src".to_string(), vec!["'self'".to_string()]),
            (
                "connect-src".to_string(),
                vec!["ipc:".to_string(), "http://ipc.localhost".to_string()]
            ),
        ]
    );
    assert_eq!(json_string(r#"{ "csp": null }"#, "csp"), None);

    for source in [
        "https:",
        "https://example.com",
        "ws://localhost:1430",
        "*",
        "http://*.localhost",
    ] {
        assert!(is_network_source(source), "{source}");
    }
    for source in [
        "'self'",
        "ipc:",
        "http://ipc.localhost",
        "data:",
        "'unsafe-inline'",
    ] {
        assert!(!is_network_source(source), "{source}");
    }
}

#[test]
fn table_reads_rows_between_its_markers() {
    let document = "text\n<!-- t:begin -->\n| A | B |\n|---|---|\n| `x/y.rs` | — |\n| z | 2 |\n<!-- t:end -->\n| not | this |";
    assert_eq!(table(document, "t"), [["x/y.rs", "—"], ["z", "2"]]);
}
