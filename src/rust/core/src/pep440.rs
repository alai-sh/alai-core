// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! ansible-core versions and `requires_ansible` specifiers, which follow PEP 440 rather than SemVer.
//!
//! ansible-core checks a collection's `requires_ansible` with `packaging.specifiers.SpecifierSet` against its own
//! version with any pre-release, post-release or dev part dropped (`_does_collection_support_ansible_version` in
//! `lib/ansible/plugins/loader.py`), which `SpecifierSet::contains` does. Local versions (`+local`) aren't supported,
//! and each number must fit in a `u64`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::ParseError;
use crate::text::number;

/// What versions compare by, as packaging's `_cmpkey` does it. The release has no trailing zeros, and the pre, post
/// and dev parts are made comparable as its infinities make them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key {
    epoch: u64,
    release: Vec<u64>,
    pre: (i8, u64),
    post: Option<u64>,
    dev: (u8, u64),
}

/// A PEP 440 version, such as `2.19.0` or `2.19.0rc1`. Equal when they compare equal, so `1.0` is `1.0.0`.
#[derive(Clone, Debug)]
pub struct Version {
    text: String,
    epoch: u64,
    release: Vec<u64>,
    is_prerelease: bool,
    is_postrelease: bool,
    key: Key,
}

const PRE_LABELS: [(&str, i8); 8] = [
    ("alpha", 0),
    ("a", 0),
    ("beta", 1),
    ("b", 1),
    ("preview", 2),
    ("pre", 2),
    ("c", 2),
    ("rc", 2),
];
const POST_LABELS: [&str; 3] = ["post", "rev", "r"];

impl Version {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        parse_version(text.trim()).ok_or_else(|| ParseError::new(format!("invalid version '{text}'")))
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_prerelease(&self) -> bool {
        self.is_prerelease
    }

    pub fn is_postrelease(&self) -> bool {
        self.is_postrelease
    }

    /// The epoch and release alone, as packaging's `base_version`. `2.19.0rc1` gives `2.19.0`.
    pub fn base(&self) -> Version {
        release_version(self.epoch, &self.release)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Version {}

impl Hash for Version {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Reads packaging's canonical pattern, case-insensitively, from the start of what's left.
struct Reader<'a> {
    text: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn eat(&mut self, literal: &str) -> bool {
        let literal = literal.as_bytes();
        let end = self.pos + literal.len();
        if self
            .text
            .get(self.pos..end)
            .is_some_and(|s| s.eq_ignore_ascii_case(literal))
        {
            self.pos = end;
            true
        } else {
            false
        }
    }

    fn separator(&mut self) {
        if matches!(self.text.get(self.pos), Some(b'-' | b'_' | b'.')) {
            self.pos += 1;
        }
    }

    /// The digits here, or None if there are none. A number too large for a `u64` is an `Err`.
    fn digits(&mut self) -> Option<Result<u64, ()>> {
        let start = self.pos;
        while self.text.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        let digits = std::str::from_utf8(&self.text[start..self.pos]).expect("ASCII digits");
        Some(number(digits).ok_or(()))
    }

    fn peek_digit(&self, offset: usize) -> bool {
        self.text.get(self.pos + offset).is_some_and(u8::is_ascii_digit)
    }
}

fn parse_version(text: &str) -> Option<Version> {
    let mut r = Reader {
        text: text.as_bytes(),
        pos: 0,
    };
    r.eat("v");

    let start = r.pos;
    let mut epoch = 0;
    if let Some(digits) = r.digits() {
        if r.eat("!") {
            epoch = digits.ok()?;
        } else {
            r.pos = start;
        }
    }

    let mut release = vec![r.digits()?.ok()?];
    while r.text.get(r.pos) == Some(&b'.') && r.peek_digit(1) {
        r.pos += 1;
        release.push(r.digits()?.ok()?);
    }

    let start = r.pos;
    r.separator();
    let mut pre = None;
    if let Some((_, rank)) = PRE_LABELS.iter().find(|(label, _)| r.eat(label)) {
        r.separator();
        pre = Some((*rank, r.digits().transpose().ok()?.unwrap_or(0)));
    } else {
        r.pos = start;
    }

    let start = r.pos;
    let mut post = None;
    if r.text.get(r.pos) == Some(&b'-') && r.peek_digit(1) {
        r.pos += 1;
        post = Some(r.digits()?.ok()?);
    } else {
        r.separator();
        if POST_LABELS.iter().any(|label| r.eat(label)) {
            r.separator();
            post = Some(r.digits().transpose().ok()?.unwrap_or(0));
        } else {
            r.pos = start;
        }
    }

    let start = r.pos;
    r.separator();
    let mut dev = None;
    if r.eat("dev") {
        r.separator();
        dev = Some(r.digits().transpose().ok()?.unwrap_or(0));
    } else {
        r.pos = start;
    }

    if r.pos != r.text.len() {
        return None;
    }

    let mut trimmed = release.clone();
    while trimmed.len() > 1 && trimmed.last() == Some(&0) {
        trimmed.pop();
    }
    // A dev release of a final version sorts before its pre-releases. Otherwise a final sorts after them.
    let pre_key = pre.unwrap_or(if post.is_none() && dev.is_some() {
        (-1, 0)
    } else {
        (3, 0)
    });
    Some(Version {
        text: text.to_owned(),
        epoch,
        release,
        is_prerelease: pre.is_some() || dev.is_some(),
        is_postrelease: post.is_some(),
        key: Key {
            epoch,
            release: trimmed,
            pre: pre_key,
            post,
            dev: dev.map_or((1, 0), |n| (0, n)),
        },
    })
}

fn release_version(epoch: u64, release: &[u64]) -> Version {
    let release = release.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
    let text = if epoch == 0 {
        release
    } else {
        format!("{epoch}!{release}")
    };
    Version::parse(&text).expect("an epoch and release are a version")
}

/// The first release after every one starting with `prefix`, such as 2.18 for 2.17 or 3 for 2.
fn next_prefix(epoch: u64, prefix: &[u64]) -> Option<Version> {
    let (last, rest) = prefix.split_last()?;
    let mut release = rest.to_vec();
    release.push(last.checked_add(1)?);
    Some(release_version(epoch, &release))
}

fn prefix_match(version: &Version, prefix: &Version) -> bool {
    if version.epoch != prefix.epoch {
        return false;
    }
    (0..prefix.release.len()).all(|i| version.release.get(i).copied().unwrap_or(0) == prefix.release[i])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Op {
    Compatible,
    Arbitrary,
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
}

// The longest operators first, so "===" isn't read as "==".
const OPS: [(&str, Op); 8] = [
    ("~=", Op::Compatible),
    ("===", Op::Arbitrary),
    ("==", Op::Eq),
    ("!=", Op::Ne),
    ("<=", Op::Le),
    (">=", Op::Ge),
    ("<", Op::Lt),
    (">", Op::Gt),
];

/// One clause of a specifier set, such as `>=2.17` or `==2.18.*`.
#[derive(Clone, Debug)]
struct Specifier {
    text: String,
    op: Op,
    /// None for `===`, which compares text and needn't be a valid version.
    version: Option<Version>,
    /// `==` or `!=` with a trailing `.*`, which matches the release by prefix.
    prefix: bool,
}

impl Specifier {
    fn parse(text: &str) -> Result<Self, ParseError> {
        let text = text.trim();
        let invalid = || ParseError::new(format!("invalid specifier '{text}'"));
        let (op_text, op) = OPS.iter().find(|(op, _)| text.starts_with(op)).ok_or_else(invalid)?;
        let version = text[op_text.len()..].trim_start();
        if version.is_empty() || version.contains(|c: char| c == ',' || c == ';' || c == ')' || c.is_whitespace()) {
            return Err(invalid());
        }
        if *op == Op::Arbitrary {
            return Ok(Specifier {
                text: text.to_owned(),
                op: *op,
                version: None,
                prefix: false,
            });
        }
        let (version, prefix) = match version.strip_suffix(".*") {
            Some(version) if matches!(op, Op::Eq | Op::Ne) => (version, true),
            _ => (version, false),
        };
        let version = Version::parse(version).map_err(|_| invalid())?;
        if prefix && (version.is_prerelease || version.is_postrelease) {
            return Err(ParseError::new(format!(
                "invalid specifier '{text}': '.*' can only follow a release such as 2.17"
            )));
        }
        if *op == Op::Compatible && version.release.len() < 2 {
            return Err(ParseError::new(format!(
                "invalid specifier '{text}': '~=' needs at least two release parts, such as ~=2.17"
            )));
        }
        Ok(Specifier {
            text: text.to_owned(),
            op: *op,
            version: Some(version),
            prefix,
        })
    }

    fn contains(&self, version: &Version) -> bool {
        let Some(bound) = &self.version else {
            return version.text.to_lowercase() == self.arbitrary();
        };
        match self.op {
            Op::Eq | Op::Ne => {
                let equal = if self.prefix {
                    prefix_match(version, bound)
                } else {
                    version == bound
                };
                equal == (self.op == Op::Eq)
            }
            Op::Compatible => {
                let prefix = release_version(bound.epoch, &bound.release[..bound.release.len() - 1]);
                version >= bound && prefix_match(version, &prefix)
            }
            Op::Le => version <= bound,
            Op::Ge => version >= bound,
            // <2.19 doesn't admit 2.19's own pre-releases, unless the bound is a pre-release itself.
            Op::Lt => {
                version < bound && (bound.is_prerelease || !version.is_prerelease || version.base() != bound.base())
            }
            // >2.19 doesn't admit 2.19's post-releases, unless the bound is a post-release itself.
            Op::Gt => {
                version > bound && (bound.is_postrelease || !version.is_postrelease || version.base() != bound.base())
            }
            Op::Arbitrary => unreachable!("=== has no version"),
        }
    }

    fn arbitrary(&self) -> String {
        self.text[3..].trim().to_lowercase()
    }
}

// Equal when they accept the same versions as written, so >=2.17 and >= 2.17.0 are one specifier.
impl PartialEq for Specifier {
    fn eq(&self, other: &Self) -> bool {
        self.op == other.op
            && self.prefix == other.prefix
            && match (&self.version, &other.version) {
                (Some(a), Some(b)) => a == b,
                (None, None) => self.arbitrary() == other.arbitrary(),
                _ => false,
            }
    }
}

impl Eq for Specifier {}

impl Hash for Specifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.op.hash(state);
        self.prefix.hash(state);
        match &self.version {
            Some(version) => version.hash(state),
            None => self.arbitrary().hash(state),
        }
    }
}

/// One end of the range a specifier set allows, as a version and whether the range includes it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bound {
    version: Version,
    inclusive: bool,
}

/// Comma-separated specifiers, all of which must hold, such as `>=2.17,<2.20`. Empty items are skipped, as packaging
/// skips them, so `>=2.17,` is `>=2.17`.
#[derive(Clone, Debug)]
pub struct SpecifierSet {
    text: String,
    specifiers: Vec<Specifier>,
}

impl SpecifierSet {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let specifiers = text
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(Specifier::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SpecifierSet {
            text: text.to_owned(),
            specifiers,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether ansible-core `version` meets every specifier. It's compared as ansible-core does, by its base
    /// version, so `2.19.0rc1` meets `>=2.19`.
    pub fn contains(&self, version: &Version) -> bool {
        let base = version.base();
        self.specifiers.iter().all(|s| s.contains(&base))
    }

    /// Whether this set starts no later than `other`, by lower bounds alone. A collection's `requires_ansible`
    /// admits the project's when it does. Either set without a lower bound admits the other.
    pub fn admits_lower_bound(&self, other: &SpecifierSet) -> bool {
        let (Some(have), Some(want)) = (self.lower_bound(), other.lower_bound()) else {
            return true;
        };
        if have.version != want.version {
            return have.version < want.version;
        }
        have.inclusive || !want.inclusive
    }

    /// Whether this set's upper bound ends inside `other`'s range, so `other` reaches versions this one doesn't.
    pub fn narrower_than(&self, other: &SpecifierSet) -> bool {
        let Some(theirs) = self.upper_bound() else {
            return false;
        };
        let Some(ours) = other.upper_bound() else {
            return true;
        };
        if ours.version != theirs.version {
            return ours.version > theirs.version;
        }
        ours.inclusive && !theirs.inclusive
    }

    /// The lowest version the set allows, or None if it has no lower bound.
    ///
    /// From `>=` and `>`, and the lower end of `~=`, `==` and `==X.*`. `!=` and `===` don't bound the range. With
    /// several, the highest wins. At one version, an exclusive bound is the higher.
    fn lower_bound(&self) -> Option<Bound> {
        let mut best: Option<Bound> = None;
        for s in &self.specifiers {
            let Some(version) = &s.version else { continue };
            let bound = match s.op {
                Op::Ge | Op::Compatible => Bound {
                    version: version.clone(),
                    inclusive: true,
                },
                Op::Eq if !s.prefix => Bound {
                    version: version.clone(),
                    inclusive: true,
                },
                Op::Gt => Bound {
                    version: version.clone(),
                    inclusive: false,
                },
                Op::Eq => Bound {
                    version: release_version(version.epoch, &version.release),
                    inclusive: true,
                },
                _ => continue,
            };
            let higher = best
                .as_ref()
                .is_none_or(|b| (&bound.version, !bound.inclusive) > (&b.version, !b.inclusive));
            if higher {
                best = Some(bound);
            }
        }
        best
    }

    /// The highest version the set allows, or None if it has no upper bound.
    ///
    /// From `<` and `<=`, and the upper end of `~=` (`~=2.17` is below 3), `==` and `==X.*` (`==2.17.*` is below
    /// 2.18). With several, the lowest wins. At one version, an exclusive bound is the lower.
    fn upper_bound(&self) -> Option<Bound> {
        let mut best: Option<Bound> = None;
        for s in &self.specifiers {
            let Some(version) = &s.version else { continue };
            let bound = match s.op {
                Op::Lt => Bound {
                    version: version.clone(),
                    inclusive: false,
                },
                Op::Le => Bound {
                    version: version.clone(),
                    inclusive: true,
                },
                Op::Eq if !s.prefix => Bound {
                    version: version.clone(),
                    inclusive: true,
                },
                Op::Compatible => match next_prefix(version.epoch, &version.release[..version.release.len() - 1]) {
                    Some(version) => Bound {
                        version,
                        inclusive: false,
                    },
                    None => continue,
                },
                Op::Eq => match next_prefix(version.epoch, &version.release) {
                    Some(version) => Bound {
                        version,
                        inclusive: false,
                    },
                    None => continue,
                },
                _ => continue,
            };
            let lower = best
                .as_ref()
                .is_none_or(|b| (&bound.version, bound.inclusive) < (&b.version, b.inclusive));
            if lower {
                best = Some(bound);
            }
        }
        best
    }
}

impl PartialEq for SpecifierSet {
    fn eq(&self, other: &Self) -> bool {
        self.specifiers == other.specifiers
    }
}

impl Eq for SpecifierSet {}

impl Hash for SpecifierSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.specifiers.hash(state);
    }
}

impl fmt::Display for SpecifierSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn set(text: &str) -> SpecifierSet {
        SpecifierSet::parse(text).unwrap()
    }

    #[test]
    fn order() {
        let ordered = [
            "1.0.dev0",
            "1.0a1.dev1",
            "1.0a1",
            "1.0b2",
            "1.0rc1",
            "1.0",
            "1.0.post1.dev0",
            "1.0.post1",
            "1.0.1",
            "2.0",
            "1!0.5",
        ];
        let mut versions: Vec<Version> = ordered.iter().rev().map(|t| v(t)).collect();
        versions.sort();
        assert_eq!(versions.iter().map(Version::text).collect::<Vec<_>>(), ordered);
    }

    #[test]
    fn equivalent() {
        for (a, b) in [
            ("1.0", "1.0.0"),
            ("1.0rc1", "1.0c1"),
            ("1.0alpha2", "1.0a2"),
            ("1.0-1", "1.0.post1"),
            ("v1.0", "1.0"),
            ("1.0A1", "1.0a1"),
            ("1.0a-1", "1.0a1"),
        ] {
            assert_eq!(v(a), v(b), "{a} {b}");
        }
    }

    #[test]
    fn invalid() {
        for text in [
            "",
            "1.0+local",
            "x",
            "1.0.x",
            "2.17.*",
            "1.0rc1rc2",
            "99999999999999999999",
        ] {
            let err = Version::parse(text).unwrap_err().to_string();
            assert!(err.starts_with("invalid version"), "{text:?}");
        }
    }

    #[test]
    fn text_and_base() {
        assert_eq!(v(" 2.17 ").text(), "2.17");
        assert_eq!(v("2.19.0rc1").base(), v("2.19.0"));
        assert_eq!(v("1!2.0.post1").base().text(), "1!2.0");
    }

    #[test]
    fn invalid_specifiers() {
        for (text, message) in [
            ("2.17", "invalid specifier '2.17'"),
            (">=2.17.*", "invalid specifier"),
            ("==2.17+local", "invalid specifier"),
            ("~=2", "needs at least two release parts"),
            ("==2.17rc1.*", "can only follow a release"),
            ("===", "invalid specifier '==='"),
        ] {
            let err = SpecifierSet::parse(text).unwrap_err().to_string();
            assert!(err.contains(message), "{text:?}: {err}");
        }
    }

    #[test]
    fn contains() {
        for (spec, version, expected) in [
            (">=2.17", "2.17.0", true),
            (">=2.17", "2.16.9", false),
            (" >= 2.15 , < 2.19 ", "2.18.3", true),
            (">=2.15,<2.19", "2.19.0", false),
            ("~=2.17", "2.20.0", true),
            ("~=2.17", "3.0.0", false),
            ("~=2.17.1", "2.17.5", true),
            ("~=2.17.1", "2.18.0", false),
            ("==2.18.*", "2.18.4", true),
            ("==2.18.*", "2.19.0", false),
            ("!=2.18.*", "2.18.0", false),
            ("==2.18", "2.18.0", true),
            ("==1!2.18.*", "2.18.0", false),
            (">2.18", "2.18.0", false),
            ("===2.18.0", "2.18.0", true),
            ("===2.18", "2.18.0", false),
            (">=2.17,", "2.17.0", true),
            ("<=2.18", "2.18.0", true),
            ("<=2.18", "2.18.1", false),
            (">=2.19.0", "2.19.0rc1", true),
            ("<2.19", "2.19.0b2", false),
            (">=2.19.0b1", "2.19.0", true),
            ("", "2.9.0", true),
        ] {
            assert_eq!(set(spec).contains(&v(version)), expected, "{spec} {version}");
        }
    }

    // ansible-core's base versions never reach these, so they're only checked here.
    #[test]
    fn exclusive_bounds_and_pre_and_post_releases() {
        for (spec, version, expected) in [
            ("<2.19", "2.19.0rc1", false),
            ("<2.19rc2", "2.19.0rc1", true),
            ("<2.19", "2.18.0rc1", true),
            (">2.18", "2.18.post1", false),
            (">2.18.post1", "2.18.post2", true),
            (">2.18", "2.19.post1", true),
        ] {
            assert_eq!(
                Specifier::parse(spec).unwrap().contains(&v(version)),
                expected,
                "{spec} {version}"
            );
        }
    }

    #[test]
    fn equality() {
        assert_eq!(set(">=2.17, <2.20"), set(">=2.17.0,<2.20"));
        assert_ne!(set(">=2.17"), set(">2.17"));
        assert_ne!(set("==2.17.*"), set("==2.17"));
        assert_eq!(set("===2.17.0"), set("=== 2.17.0"));
    }

    #[test]
    fn admits_lower_bound() {
        for (collection, project, expected) in [
            (">=2.15", ">=2.17", true),
            (">=2.17", ">=2.17", true),
            (">=2.18", ">=2.17", false),
            (">2.17", ">=2.17", false),
            (">2.17", ">2.17", true),
            (">=2.17", ">2.17", true),
            // Upper bounds are ignored.
            (">=2.15,<2.16", ">=2.17", true),
            ("<2.20", ">=2.17", true),
            // A project with no lower bound filters nothing.
            (">=2.99", "<2.20", true),
            ("~=2.16", ">=2.17", true),
            ("==2.18.*", ">=2.17", false),
        ] {
            assert_eq!(
                set(collection).admits_lower_bound(&set(project)),
                expected,
                "{collection} {project}"
            );
        }
    }

    #[test]
    fn narrower_than() {
        for (collection, project, expected) in [
            (">=2.15", ">=2.17", false),
            ("<2.20", ">=2.17", true),
            ("<2.20", ">=2.17,<2.20", false),
            ("<2.20", ">=2.17,<=2.20", true),
            ("<=2.20", ">=2.17,<=2.20", false),
            ("<2.19", ">=2.17,<2.20", true),
            ("~=2.15", ">=2.17", true),
            ("<2.20", ">=2.17,<2.21", true),
        ] {
            assert_eq!(
                set(collection).narrower_than(&set(project)),
                expected,
                "{collection} {project}"
            );
        }
    }

    #[test]
    fn bounds() {
        let bound = |b: Option<(&str, bool)>| {
            b.map(|(version, inclusive)| Bound {
                version: v(version),
                inclusive,
            })
        };
        for (spec, lower, upper) in [
            ("", None, None),
            (">=2.17", Some(("2.17", true)), None),
            (">2.16", Some(("2.16", false)), None),
            ("<2.20", None, Some(("2.20", false))),
            ("<=2.19", None, Some(("2.19", true))),
            (">=2.15,<2.19", Some(("2.15", true)), Some(("2.19", false))),
            ("~=2.17", Some(("2.17", true)), Some(("3", false))),
            ("~=2.17.1", Some(("2.17.1", true)), Some(("2.18", false))),
            ("==2.17.*", Some(("2.17", true)), Some(("2.18", false))),
            ("==2.17.3", Some(("2.17.3", true)), Some(("2.17.3", true))),
            ("!=2.18.0", None, None),
            ("===2.18.0", None, None),
            (">=2.15,>=2.17,<2.20,<2.19", Some(("2.17", true)), Some(("2.19", false))),
            (">=2.17,>2.17", Some(("2.17", false)), None),
            ("<2.19,<=2.19", None, Some(("2.19", false))),
            (">=1!2.0", Some(("1!2.0", true)), None),
            ("==1!2.17.*", Some(("1!2.17", true)), Some(("1!2.18", false))),
        ] {
            assert_eq!(set(spec).lower_bound(), bound(lower), "{spec}");
            assert_eq!(set(spec).upper_bound(), bound(upper), "{spec}");
        }
    }
}
