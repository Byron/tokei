use std::{ops::Range, path::Path};

use once_cell::sync::Lazy;
use regex::bytes::Regex;

pub(super) fn is_test_path(path: &Path) -> bool {
    let absolute = std::env::current_dir().ok().map(|cwd| cwd.join(path));
    let path = absolute.as_deref().unwrap_or(path);
    let mut parents = 0;
    path.components().rev().any(|component| match component {
        std::path::Component::ParentDir => {
            parents += 1;
            false
        }
        std::path::Component::Normal(_) if parents > 0 => {
            parents -= 1;
            false
        }
        std::path::Component::Normal(name) => name == "tests",
        _ => false,
    })
}

/// Physical lines belonging to items with test-only attributes.
pub(super) fn ranges(text: &[u8]) -> Vec<Range<usize>> {
    if !text.contains(&b'#') {
        return Vec::new();
    }
    // shortcut: only explicit test attributes, evaluate cfg expressions when compound conditions are needed.
    static TEST_ATTRIBUTE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?s)^\s*(?:cfg\s*\(\s*test\s*\)|(?:[A-Za-z_]\w*\s*::\s*)*test(?:\s*\(.*\))?)\s*$",
        )
        .unwrap()
    });

    let mut ranges = Vec::new();
    let mut attributes = None;
    let mut is_test = false;
    let mut i = 0;
    while i < text.len() {
        if text[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if let Some(end) = skip_literal_or_comment(text, i) {
            // Comments can appear between attributes and their item.
            if !text[i..].starts_with(b"//") && !text[i..].starts_with(b"/*") {
                attributes = None;
                is_test = false;
            }
            i = end;
            continue;
        }
        if text[i] == b'#' {
            let mut bracket = i + 1;
            while text.get(bracket).is_some_and(u8::is_ascii_whitespace) {
                bracket += 1;
            }
            if text.get(bracket) == Some(&b'[') {
                if let Some(end) = item_end(text, bracket) {
                    attributes.get_or_insert(i);
                    is_test |= TEST_ATTRIBUTE.is_match(&text[bracket + 1..end - 1]);
                    i = end;
                    continue;
                }
            }
        }
        if let Some(start) = attributes.take() {
            if is_test {
                let end = item_end(text, i).unwrap_or(text.len());
                let line_start = text[..start]
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map_or(0, |p| p + 1);
                let line_end = text[end..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(text.len(), |p| end + p + 1);
                ranges.push(line_start..line_end);
                i = end;
                is_test = false;
                continue;
            }
            is_test = false;
        }
        i += 1;
    }
    ranges
}

/// Find an attribute's closing bracket or an item's closing brace/semicolon.
fn item_end(text: &[u8], start: usize) -> Option<usize> {
    let mut delimiters = Vec::new();
    let mut i = start;
    while i < text.len() {
        if let Some(end) = skip_literal_or_comment(text, i) {
            i = end;
            continue;
        }
        match text[i] {
            b'(' => delimiters.push(b')'),
            b'[' => delimiters.push(b']'),
            b'{' => delimiters.push(b'}'),
            b')' | b']' | b'}' => {
                if delimiters.pop() != Some(text[i]) {
                    return None;
                }
                if delimiters.is_empty() && (text[i] == b'}' || text[start] == b'[') {
                    return Some(i + 1);
                }
            }
            b';' if delimiters.is_empty() => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_literal_or_comment(text: &[u8], start: usize) -> Option<usize> {
    let rest = &text[start..];
    if rest.starts_with(b"//") {
        return Some(
            rest.iter()
                .position(|&b| b == b'\n')
                .map_or(text.len(), |p| start + p),
        );
    }
    if rest.starts_with(b"/*") {
        let mut depth = 1;
        let mut i = start + 2;
        while i + 1 < text.len() {
            if text[i..].starts_with(b"/*") {
                depth += 1;
                i += 2;
            } else if text[i..].starts_with(b"*/") {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return Some(i);
                }
            } else {
                i += 1;
            }
        }
        return Some(text.len());
    }
    if rest[0] == b'r' {
        let hashes = rest[1..].iter().take_while(|&&b| b == b'#').count();
        if rest.get(1 + hashes) == Some(&b'"') {
            let mut i = start + 2 + hashes;
            while i < text.len() {
                if text[i] == b'"' && text.get(i + 1..i + 1 + hashes) == Some(&rest[1..1 + hashes])
                {
                    return Some(i + 1 + hashes);
                }
                i += 1;
            }
            return Some(text.len());
        }
    }
    if rest[0] == b'"' || rest.starts_with(b"'\\") {
        let quote = rest[0];
        let mut i = start + 1;
        while i < text.len() {
            if text[i] == b'\\' {
                i += 2;
            } else if text[i] == quote {
                return Some(i + 1);
            } else {
                i += 1;
            }
        }
        return Some(text.len());
    }
    if rest[0] == b'\'' {
        // A lifetime has no closing quote immediately after one character.
        let width = match rest.get(1)? {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        if rest.get(1 + width) == Some(&b'\'') {
            return Some(start + width + 2);
        }
    }
    None
}
