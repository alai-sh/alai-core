// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use crate::ParseError;

// Python's keyword.kwlist, less True, False and None, which can't match a lowercase name.
const KEYWORDS: [&str; 32] = [
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
    "return", "try", "while", "with", "yield",
];

/// Check `text` is a valid `namespace.name`. Each part is a lowercase Python identifier that isn't a keyword, as
/// ansible-core's `is_valid_collection_name` and the index spec require.
pub fn check_collection_name(text: &str) -> Result<(), ParseError> {
    let mut parts = text.split('.');
    let (Some(namespace), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ParseError::new(format!(
            "invalid collection name '{text}', expected namespace.name"
        )));
    };
    for part in [namespace, name] {
        let mut chars = part.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !KEYWORDS.contains(&part);
        if !valid {
            let hint = if part.to_lowercase() != part {
                ", collection names are lowercase"
            } else {
                ""
            };
            return Err(ParseError::new(format!(
                "invalid collection name '{text}': '{part}' must be a Python identifier that isn't a keyword{hint}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_names() {
        for valid in ["community.general", "ns.coll", "_ns.coll_2", "a.b", "ns.async_tasks"] {
            assert!(check_collection_name(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "community",
            "a.b.c",
            ".coll",
            "ns.",
            "1ns.coll",
            "ns.2coll",
            "ns.co-ll",
            "ns.co ll",
            "ns.class",
            "async.coll",
            "ns.collé",
        ] {
            assert!(check_collection_name(invalid).is_err(), "{invalid}");
        }
        let upper = check_collection_name("Ns.coll").unwrap_err().to_string();
        assert!(upper.ends_with("collection names are lowercase"), "{upper}");
    }
}
