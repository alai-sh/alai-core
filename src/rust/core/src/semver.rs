// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! Collection versions and version requirements, compared as ansible-galaxy compares them.
//!
//! Versions are SemVer 2.0.0, as `ansible.utils.version.SemanticVersion` parses them, but ASCII only, with each
//! number fitting in a `u64`, as the `semver` crate parses them. Requirements are written either in `alai.toml`'s
//! strict form or in ansible-galaxy's syntax (`meets_requirements`), which an index's `deps[].req` and a collection's
//! `MANIFEST.json` use.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::ParseError;
use crate::text::number;

/// SemVer precedence, which is what requirements compare. Build metadata isn't part of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Precedence(semver::Version);

impl fmt::Display for Precedence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A collection version, kept as written. Versions are equal by text. They're ordered by precedence and then by
/// text, so two that differ only in build metadata still have an order.
#[derive(Clone, Debug)]
pub struct Version {
    text: String,
    precedence: Precedence,
}

impl Version {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut parsed = semver::Version::parse(text).map_err(|_| {
            ParseError::new(format!(
                "invalid version '{text}', expected a semantic version such as 1.2.3"
            ))
        })?;
        parsed.build = semver::BuildMetadata::EMPTY;
        Ok(Version {
            text: text.to_owned(),
            precedence: Precedence(parsed),
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn precedence(&self) -> &Precedence {
        &self.precedence
    }

    pub fn is_prerelease(&self) -> bool {
        !self.precedence.0.pre.is_empty()
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for Version {}

impl Hash for Version {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.precedence, &self.text).cmp(&(&other.precedence, &other.text))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

impl Op {
    pub fn parse(text: &str) -> Option<Op> {
        Some(match text {
            "==" => Op::Eq,
            "!=" => Op::Ne,
            ">=" => Op::Ge,
            "<=" => Op::Le,
            ">" => Op::Gt,
            "<" => Op::Lt,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Ge => ">=",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Lt => "<",
        }
    }

    fn holds(self, version: &Precedence, bound: &Precedence) -> bool {
        match self {
            Op::Eq => version == bound,
            Op::Ne => version != bound,
            Op::Ge => version >= bound,
            Op::Le => version <= bound,
            Op::Gt => version > bound,
            Op::Lt => version < bound,
        }
    }
}

/// The versions a requirement accepts. Every clause must hold, and no clauses accepts any version. Requirements are
/// equal by clauses, so `>= 1` and `>=1.0.0` are the same requirement.
#[derive(Clone, Debug)]
pub struct VersionReq {
    text: String,
    clauses: Vec<(Op, Version)>,
}

// The longest operators first, so ">=" isn't read as ">".
const STRICT_OPS: [&str; 6] = ["==", "!=", ">=", "<=", ">", "<"];
const GALAXY_OPS: [&str; 7] = ["==", "!=", ">=", "<=", "=", ">", "<"];

impl VersionReq {
    /// A requirement as `alai.toml` takes it. That's `*`, or comma-separated clauses of an operator and a version,
    /// which may leave out its minor and patch parts (`>=9` is `>=9.0.0`). This is ansible-galaxy's syntax without
    /// the parts that read as something else, such as a bare version, `=`, build metadata and anything it would parse
    /// loosely.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        if text.trim() == "*" {
            return Ok(VersionReq {
                text: text.to_owned(),
                clauses: Vec::new(),
            });
        }
        let clauses = text
            .split(',')
            .map(|clause| strict_clause(clause).ok_or_else(|| clause_error(text, clause.trim())))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VersionReq {
            text: text.to_owned(),
            clauses,
        })
    }

    /// A requirement as ansible-galaxy reads one, quirks included. There's no whitespace, a clause without an
    /// operator means `==`, and `*` as any clause matches anything. The version is read with `LooseVersion` and then
    /// cut to three parts, so `1.2.3.4` means `1.2.3`. ansible-galaxy only fails on a bad clause once the clauses
    /// before it match, so `>=1.0.0,` is merely unmet for `0.9.0` there. Here it's an error for every version.
    pub fn parse_galaxy(text: &str) -> Result<Self, ParseError> {
        let mut clauses = Vec::new();
        for clause in text.split(',') {
            let op = GALAXY_OPS.iter().find(|op| clause.starts_with(**op)).copied();
            let version = &clause[op.map_or(0, str::len)..];
            if version == "*" {
                continue;
            }
            let version = from_loose(version).ok_or_else(|| {
                ParseError::new(format!("invalid version requirement '{text}': can't read '{clause}'"))
            })?;
            let op = match op {
                None | Some("=") => Op::Eq,
                Some(op) => Op::parse(op).expect("every other galaxy operator is a strict one"),
            };
            clauses.push((op, version));
        }
        Ok(VersionReq {
            text: text.to_owned(),
            clauses,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn clauses(&self) -> &[(Op, Version)] {
        &self.clauses
    }

    pub fn contains(&self, version: &Version) -> bool {
        self.clauses
            .iter()
            .all(|(op, bound)| op.holds(&version.precedence, &bound.precedence))
    }
}

impl PartialEq for VersionReq {
    fn eq(&self, other: &Self) -> bool {
        self.clauses == other.clauses
    }
}

impl Eq for VersionReq {}

impl Hash for VersionReq {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.clauses.hash(state);
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// An operator and a version of one to three parts with an optional pre-release, whitespace allowed around each.
fn strict_clause(clause: &str) -> Option<(Op, Version)> {
    let clause = clause.trim();
    let op = STRICT_OPS.iter().find(|op| clause.starts_with(**op))?;
    let version = clause[op.len()..].trim();
    if version.contains('+') {
        return None;
    }
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => (core, format!("-{prerelease}")),
        None => (version, String::new()),
    };
    let parts = core.split('.').count();
    if parts > 3 {
        return None;
    }
    let padding = ".0".repeat(3 - parts);
    Some((
        Op::parse(op)?,
        Version::parse(&format!("{core}{padding}{prerelease}")).ok()?,
    ))
}

fn clause_error(text: &str, clause: &str) -> ParseError {
    let start = format!("invalid version requirement '{text}'");
    let message = if clause.is_empty() {
        format!("{start}: empty clause")
    } else if clause == "*" {
        format!("{start}: '*' can't be combined with other clauses")
    } else if clause.starts_with('^') || clause.starts_with('~') {
        let range = &clause[..1];
        format!("{start}: '{range}' ranges aren't supported, write the bounds, such as '>=1.2.0,<2.0.0'")
    } else if clause.starts_with("=>") || clause.starts_with("=<") {
        let op = &clause[1..2];
        let wrong = &clause[..2];
        format!("{start}: use '{op}=' rather than '{wrong}' in '{clause}'")
    } else if clause.starts_with('=') && !clause.starts_with("==") {
        format!("{start}: use '==' rather than '=' in '{clause}'")
    } else if clause.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{start}: '{clause}' needs an operator, such as '=={clause}' for exactly that version")
    } else if clause.contains('+') {
        format!("{start}: '{clause}' has build metadata, which version comparisons ignore")
    } else {
        format!("{start}: expected an operator (==, !=, >=, <=, >, <) and a version such as 1.2.3, got '{clause}'")
    };
    ParseError::new(message)
}

#[derive(Debug, PartialEq)]
enum Component<'a> {
    Number(u64),
    Text(&'a str),
}

/// `SemanticVersion.from_loose_version(LooseVersion(text))`.
fn from_loose(text: &str) -> Option<Version> {
    if text.is_empty() {
        return None;
    }
    // LooseVersion splits on runs of digits, runs of lowercase letters and dots, keeping what's between them, then
    // drops the dots.
    let mut components = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind = |b: u8| {
            if b.is_ascii_digit() {
                0
            } else if b.is_ascii_lowercase() {
                1
            } else if b == b'.' {
                2
            } else {
                3
            }
        };
        let current = kind(bytes[i]);
        i += 1;
        if current != 2 {
            while i < bytes.len() && kind(bytes[i]) == current {
                i += 1;
            }
        }
        // A run of other bytes may end inside a character, so extend it to the character's end.
        while !text.is_char_boundary(i) {
            i += 1;
        }
        let part = &text[start..i];
        match current {
            0 => components.push(Component::Number(number(part)?)),
            2 => {}
            _ => components.push(Component::Text(part)),
        }
    }

    // The core is the first three components, or fewer if a "-" or "+" comes first.
    let marker = components
        .iter()
        .position(|c| matches!(c, Component::Text("-") | Component::Text("+")));
    let end = marker.map_or(3, |m| m.min(3)).min(components.len());
    let mut numbers = components[..end]
        .iter()
        .map(|c| match c {
            Component::Number(n) => Some(*n),
            Component::Text(_) => None,
        })
        .collect::<Option<Vec<u64>>>()?;
    numbers.resize(3, 0);

    let mut padded = format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]);
    padded.push_str(loose_extra(text));
    Version::parse(&padded).ok()
}

/// What the regex `[+-].+$` finds. That's from the first "+" or "-" with more on its line after it, to the end of
/// that line, and only the last line may be followed by a newline.
fn loose_extra(text: &str) -> &str {
    for (i, c) in text.char_indices() {
        if c != '+' && c != '-' {
            continue;
        }
        let rest = &text[i + 1..];
        let body = rest.strip_suffix('\n').unwrap_or(rest);
        if !body.is_empty() && !body.contains('\n') {
            return &text[i..i + 1 + body.len()];
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn clauses(req: &VersionReq) -> Vec<(&'static str, &str)> {
        req.clauses().iter().map(|(op, v)| (op.as_str(), v.text())).collect()
    }

    #[test]
    fn invalid_versions() {
        for text in [
            "1.0",
            "v1.0.0",
            "1.0.0.0",
            "01.0.0",
            "1.0.0-",
            "1.0.0-01",
            "1.0.0+",
            "",
            "1.0.0-a..b",
        ] {
            assert!(Version::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn outside_ascii_and_u64() {
        // Python's \d would take other scripts' digits, and its int any size. Neither is SemVer.
        assert!(Version::parse("1\u{0660}.0.0").is_err());
        assert!(Version::parse("1.0.0-\u{0663}a").is_err());
        assert!(Version::parse("18446744073709551616.0.0").is_err());
        assert!(Version::parse("18446744073709551615.0.0").is_ok());
    }

    #[test]
    fn precedence_order() {
        // The order in the SemVer 2.0.0 spec, item 11.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
            "10.0.0",
        ];
        let mut versions: Vec<Version> = ordered.iter().rev().map(|t| v(t)).collect();
        versions.sort();
        assert_eq!(versions.iter().map(Version::text).collect::<Vec<_>>(), ordered);
    }

    #[test]
    fn build_metadata_sorts_by_text() {
        let mut versions = vec![v("1.0.0+b"), v("1.0.0"), v("1.0.0+a")];
        versions.sort();
        assert_eq!(
            versions.iter().map(Version::text).collect::<Vec<_>>(),
            ["1.0.0", "1.0.0+a", "1.0.0+b"]
        );
        assert_ne!(v("1.0.0+a"), v("1.0.0+b"));
        assert_eq!(v("1.0.0+a").precedence(), v("1.0.0+b").precedence());
    }

    #[test]
    fn prerelease() {
        assert!(v("1.0.0-rc.1").is_prerelease());
        assert!(!v("1.0.0+build").is_prerelease());
        assert_eq!(v("1.0.0-rc.1+b").precedence().to_string(), "1.0.0-rc.1");
    }

    #[test]
    fn strict_requirements() {
        for (text, expected) in [
            ("*", vec![]),
            (" * ", vec![]),
            (">=9", vec![(">=", "9.0.0")]),
            (">=9,<11", vec![(">=", "9.0.0"), ("<", "11.0.0")]),
            (" >= 9.1 , < 11 ", vec![(">=", "9.1.0"), ("<", "11.0.0")]),
            ("==1.2.3", vec![("==", "1.2.3")]),
            ("!=1.2.3-rc.1", vec![("!=", "1.2.3-rc.1")]),
            (">2.0-beta", vec![(">", "2.0.0-beta")]),
            ("<=0.1", vec![("<=", "0.1.0")]),
        ] {
            let req = VersionReq::parse(text).unwrap();
            assert_eq!(clauses(&req), expected, "{text:?}");
            assert_eq!(req.to_string(), text);
        }
    }

    #[test]
    fn strict_requirement_errors() {
        for (text, message) in [
            ("", "empty clause"),
            (">=1,", "empty clause"),
            ("*,>=1", "'*' can't be combined"),
            ("^1.0.0", "'^' ranges aren't supported"),
            ("~1.0", "'~' ranges aren't supported"),
            ("=1.0.0", "use '==' rather than '='"),
            ("1.0.0", "'1.0.0' needs an operator, such as '==1.0.0'"),
            ("==1.0.0+build", "has build metadata"),
            ("=>1.0", "use '>=' rather than '=>'"),
            (">=1.2.3.4", "expected an operator"),
            (">=v1", "expected an operator"),
            (">=01", "expected an operator"),
        ] {
            let err = VersionReq::parse(text).unwrap_err().to_string();
            assert!(err.contains(message), "{text:?}: {err}");
        }
    }

    #[test]
    fn galaxy_requirements() {
        for (text, expected) in [
            ("*", vec![]),
            (">=1.0.0", vec![(">=", "1.0.0")]),
            ("1.0", vec![("==", "1.0.0")]),
            ("=1.0.0", vec![("==", "1.0.0")]),
            (">1.0,<2", vec![(">", "1.0.0"), ("<", "2.0.0")]),
            ("*,>=2", vec![(">=", "2.0.0")]),
            ("1.2.3.4", vec![("==", "1.2.3")]),
            ("1.0-rc1", vec![("==", "1.0.0-rc1")]),
            ("1-rc.1", vec![("==", "1.0.0-rc.1")]),
            ("1.0.0-RC1", vec![("==", "1.0.0-RC1")]),
            ("01.2", vec![("==", "1.2.0")]),
            (">=1.0.0-beta+build", vec![(">=", "1.0.0-beta+build")]),
            ("-1", vec![("==", "0.0.0-1")]),
        ] {
            assert_eq!(clauses(&VersionReq::parse_galaxy(text).unwrap()), expected, "{text:?}");
        }
    }

    #[test]
    fn galaxy_requirement_errors() {
        for text in ["^3.0.0", "=>1.0", " >=1.0", ">= 1.0", ">=1.0.0,", "", "v1.0", "1.0-RC1"] {
            let err = VersionReq::parse_galaxy(text).unwrap_err().to_string();
            assert!(err.starts_with("invalid version requirement"), "{text:?}: {err}");
        }
    }

    #[test]
    fn contains() {
        for (req, version, expected) in [
            ("*", "0.0.1-alpha", true),
            (">=1.0.0", "1.0.0", true),
            (">=1.0.0", "1.0.0-rc.1", false),
            (">=1.0.0", "2.0.0-rc.1", true),
            ("<2.0.0", "2.0.0-rc.1", true),
            (">1.0.0,<2.0.0", "1.5.0", true),
            (">1.0.0,<2.0.0", "2.0.0", false),
            ("!=1.2.3", "1.2.3", false),
            ("!=1.2.3", "1.2.4", true),
            ("==1.0.0", "1.0.0+build", true),
            ("1.0.0-beta.11", "1.0.0-beta.2", false),
            (">1.0.0-beta.2", "1.0.0-beta.11", true),
        ] {
            assert_eq!(
                VersionReq::parse_galaxy(req).unwrap().contains(&v(version)),
                expected,
                "{req} {version}"
            );
        }
    }

    #[test]
    fn equal_by_clauses() {
        assert_eq!(
            VersionReq::parse(">= 1").unwrap(),
            VersionReq::parse_galaxy(">=1.0.0").unwrap()
        );
        assert_eq!(
            VersionReq::parse("*").unwrap(),
            VersionReq::parse_galaxy("*,*").unwrap()
        );
    }
}
