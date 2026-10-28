// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! The alai index format, read and written, as galaxy-sync's `docs/index-spec/README.md` specifies it.
//!
//! Readers skip what they can't use and say why, as the spec's client rules ask. Writers are strict, so nothing
//! written here is something a reader would skip.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde_json::{Map, Value};

use crate::ParseError;
use crate::json::{is_blank, object, string};
use crate::names::check_collection_name;
use crate::pep440::SpecifierSet;
use crate::semver::{Version, VersionReq};

/// The highest `config.json` format read and the one written.
pub const FORMAT: u64 = 1;
/// The `collections.json` `v` read and written.
pub const COLLECTIONS_VERSION: u64 = 1;
/// The index line `v` read and written.
pub const LINE_VERSION: u64 = 1;

const DL_PLACEHOLDERS: [&str; 5] = [
    "{sha256-checksum}",
    "{sha256-prefix}",
    "{namespace}",
    "{name}",
    "{version}",
];

/// Why a file can't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    Invalid(String),
    /// Only newer formats or versions than this code reads, listed in `found`.
    Unsupported {
        found: Vec<u64>,
        supported: u64,
    },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Invalid(message) => f.write_str(message),
            FormatError::Unsupported { found, supported } => {
                let found = found.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
                let found = if found.is_empty() { "none".to_owned() } else { found };
                write!(f, "offers format {found}, and only {supported} is supported")
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// An index's `config.json`, with its paths as given, relative to the index root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    format: u64,
    index: String,
    collections: Option<String>,
    plugins: Option<String>,
    dl: String,
    moved_to: Option<String>,
}

impl Config {
    pub fn new(
        index: &str,
        collections: Option<&str>,
        plugins: Option<&str>,
        dl: &str,
        moved_to: Option<&str>,
    ) -> Result<Self, ParseError> {
        Self::build(FORMAT, index, collections, plugins, dl, moved_to).map_err(ParseError::new)
    }

    /// An empty path is the same as none.
    fn build(
        format: u64,
        index: &str,
        collections: Option<&str>,
        plugins: Option<&str>,
        dl: &str,
        moved_to: Option<&str>,
    ) -> Result<Self, String> {
        let given = |s: Option<&str>| s.filter(|s| !s.is_empty()).map(str::to_owned);
        let config = Config {
            format,
            index: index.to_owned(),
            collections: given(collections),
            plugins: given(plugins),
            dl: dl.to_owned(),
            moved_to: given(moved_to),
        };
        config.check()?;
        Ok(config)
    }

    /// The highest format offered that's supported, and where the index has moved to, if it has.
    pub fn parse(data: &[u8]) -> Result<Self, FormatError> {
        let invalid = |message: String| FormatError::Invalid(message);
        let obj = object(data).map_err(invalid)?;
        let Some(Value::Object(formats)) = obj.get("formats") else {
            return Err(invalid("'formats' must be an object".to_owned()));
        };
        let mut offered: Vec<(u64, &Value)> = formats
            .iter()
            .filter(|(key, _)| !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()))
            .filter_map(|(key, paths)| key.parse().ok().map(|n| (n, paths)))
            .collect();
        offered.sort_by_key(|(n, _)| *n);
        let Some(&(number, paths)) = offered.iter().rfind(|(n, _)| *n <= FORMAT) else {
            return Err(FormatError::Unsupported {
                found: offered.iter().map(|(n, _)| *n).collect(),
                supported: FORMAT,
            });
        };
        let Value::Object(paths) = paths else {
            return Err(invalid(format!("'formats.{number}' must be an object")));
        };
        let Some(Value::String(index)) = paths.get("index") else {
            return Err(invalid(format!("'formats.{number}' must have an 'index' template")));
        };
        let path = |key: &str| match paths.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(path)) => Ok(Some(path.as_str())),
            Some(_) => Err(invalid(format!("'formats.{number}.{key}' must be a path"))),
        };
        let collections = path("collections")?;
        let plugins = path("plugins")?;
        let dl = match obj.get("dl") {
            Some(Value::String(dl)) => dl.as_str(),
            _ => return Err(invalid(dl_error())),
        };
        let moved_to = match obj.get("moved-to") {
            None | Some(Value::Null) => None,
            Some(Value::String(url)) => Some(url.as_str()),
            Some(_) => return Err(invalid("'moved-to' must be a URL or null".to_owned())),
        };
        Self::build(number, index, collections, plugins, dl, moved_to).map_err(invalid)
    }

    fn check(&self) -> Result<(), String> {
        let n = self.format;
        let index = &self.index;
        if !(index.contains("{namespace}") && index.contains("{name}")) && !index.contains("{sha256-checksum}") {
            return Err(format!(
                "'formats.{n}.index' must be a URL template naming each file, with {{namespace}} and {{name}}, or \
                 {{sha256-checksum}}"
            ));
        }
        if self.collections.is_none() && self.is_hashed() {
            return Err(format!(
                "'formats.{n}.index' finds files by SHA-256, which only a 'collections' listing gives"
            ));
        }
        if !DL_PLACEHOLDERS.iter().any(|p| self.dl.contains(p)) {
            return Err(dl_error());
        }
        Ok(())
    }

    pub fn format(&self) -> u64 {
        self.format
    }

    /// The template for each collection's index file.
    pub fn index(&self) -> &str {
        &self.index
    }

    /// Where `collections.json` is, or None if the index has no listing.
    pub fn collections(&self) -> Option<&str> {
        self.collections.as_deref()
    }

    /// Where the plugin index is, or None if the index has none.
    pub fn plugins(&self) -> Option<&str> {
        self.plugins.as_deref()
    }

    /// The template for artifact URLs.
    pub fn dl(&self) -> &str {
        &self.dl
    }

    pub fn moved_to(&self) -> Option<&str> {
        self.moved_to.as_deref()
    }

    /// Whether index files are found by their SHA-256, which only `collections.json` gives.
    pub fn is_hashed(&self) -> bool {
        self.index.contains("{sha256-checksum}") || self.index.contains("{sha256-prefix}")
    }

    /// Where collection `name`'s index file is, relative to the index root.
    ///
    /// `sha256` is the SHA-256 of the index file itself, from its entry in `collections.json`. When `is_hashed` is
    /// true it's required, and None is an error. Otherwise the template doesn't use it, and it's ignored.
    pub fn index_path(&self, name: &str, sha256: Option<&str>) -> Result<String, ParseError> {
        check_collection_name(name)?;
        if self.is_hashed() && sha256.is_none() {
            return Err(ParseError::new(format!(
                "{name} has no sha256, which the index template needs"
            )));
        }
        let (namespace, short) = name.split_once('.').expect("a checked name has a dot");
        let sha256 = sha256.unwrap_or("");
        Ok(self
            .index
            .replace("{namespace}", namespace)
            .replace("{name}", short)
            .replace("{sha256-checksum}", sha256)
            .replace("{sha256-prefix}", sha256.get(..2).unwrap_or(sha256)))
    }

    /// Where an artifact is, from the `dl` template. `sha256` is the artifact's, as an index line gives it.
    pub fn artifact_path(&self, name: &str, version: &Version, sha256: &str) -> Result<String, ParseError> {
        check_collection_name(name)?;
        let (namespace, short) = name.split_once('.').expect("a checked name has a dot");
        Ok(self
            .dl
            .replace("{sha256-checksum}", sha256)
            .replace("{sha256-prefix}", sha256.get(..2).unwrap_or(sha256))
            .replace("{namespace}", namespace)
            .replace("{name}", short)
            .replace("{version}", &percent_encode(version.text())))
    }

    pub fn to_json(&self) -> String {
        let mut paths = vec![format!("\"index\": {}", string(&self.index))];
        if let Some(collections) = &self.collections {
            paths.push(format!("\"collections\": {}", string(collections)));
        }
        if let Some(plugins) = &self.plugins {
            paths.push(format!("\"plugins\": {}", string(plugins)));
        }
        let moved_to = self.moved_to.as_deref().map_or("null".to_owned(), string);
        format!(
            "{{\n  \"formats\": {{\n    \"{}\": {{\n      {}\n    }}\n  }},\n  \"dl\": {},\n  \"moved-to\": {}\n}}\n",
            self.format,
            paths.join(",\n      "),
            string(&self.dl),
            moved_to,
        )
    }
}

fn dl_error() -> String {
    "'dl' must be a URL template with a placeholder such as {sha256-checksum}".to_owned()
}

/// `urllib.parse.quote(text, safe="")`, for a version in a URL. SemVer's `+` is the character this changes.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A collection's entry in `collections.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionEntry {
    updated: String,
    sha256: Option<String>,
}

impl CollectionEntry {
    pub fn new(updated: &str, sha256: Option<&str>) -> Result<Self, ParseError> {
        check_time("updated", updated)?;
        if let Some(sha256) = sha256 {
            check_sha256("sha256", sha256)?;
        }
        Ok(CollectionEntry {
            updated: updated.to_owned(),
            sha256: sha256.map(str::to_owned),
        })
    }

    /// When its index file last changed, or empty if the index didn't say.
    pub fn updated(&self) -> &str {
        &self.updated
    }

    /// The SHA-256 of its index file, or None if the index doesn't publish one.
    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }
}

/// An index's `collections.json`, each collection's entry by `namespace.name`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Collections {
    collections: BTreeMap<String, CollectionEntry>,
}

impl Collections {
    pub fn new(collections: impl IntoIterator<Item = (String, CollectionEntry)>) -> Result<Self, ParseError> {
        let collections: BTreeMap<String, CollectionEntry> = collections.into_iter().collect();
        for name in collections.keys() {
            check_collection_name(name)?;
        }
        Ok(Collections { collections })
    }

    /// A missing or malformed `updated` or `sha256` is left out rather than an error. Neither is needed to resolve,
    /// and without `sha256` a client revalidates the index file as usual.
    pub fn parse(data: &[u8]) -> Result<Self, FormatError> {
        let obj = object(data).map_err(FormatError::Invalid)?;
        let version = obj.get("v").and_then(Value::as_u64);
        if let Some(v) = version.filter(|v| *v > COLLECTIONS_VERSION) {
            return Err(FormatError::Unsupported {
                found: vec![v],
                supported: COLLECTIONS_VERSION,
            });
        }
        let (Some(COLLECTIONS_VERSION), Some(Value::Object(namespaces))) = (version, obj.get("namespaces")) else {
            return Err(FormatError::Invalid(format!(
                "expected 'v': {COLLECTIONS_VERSION} and a 'namespaces' object"
            )));
        };
        let mut collections = BTreeMap::new();
        for (namespace, entries) in namespaces {
            let Value::Object(entries) = entries else {
                return Err(FormatError::Invalid(format!(
                    "namespace '{namespace}' must be an object"
                )));
            };
            for (name, entry) in entries {
                let field = |key: &str| entry.as_object().and_then(|e| e.get(key)).and_then(Value::as_str);
                let listed = CollectionEntry {
                    updated: field("updated").unwrap_or("").to_owned(),
                    sha256: field("sha256").filter(|s| is_sha256(s)).map(str::to_owned),
                };
                collections.insert(format!("{namespace}.{name}"), listed);
            }
        }
        Ok(Collections { collections })
    }

    pub fn get(&self, name: &str) -> Option<&CollectionEntry> {
        self.collections.get(name)
    }

    /// Every collection, by name in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &CollectionEntry)> {
        self.collections.iter().map(|(name, listed)| (name.as_str(), listed))
    }

    pub fn len(&self) -> usize {
        self.collections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }

    pub fn to_json(&self) -> Result<String, ParseError> {
        let mut namespaces: BTreeMap<&str, Vec<(&str, &CollectionEntry)>> = BTreeMap::new();
        for (name, listed) in &self.collections {
            check_collection_name(name)?;
            check_time("updated", &listed.updated).map_err(|e| ParseError::new(format!("{name}: {e}")))?;
            let (namespace, short) = name.split_once('.').expect("a listed name has a dot");
            namespaces.entry(namespace).or_default().push((short, listed));
        }
        let mut out = format!("{{\n \"v\": {COLLECTIONS_VERSION},\n \"namespaces\": {{");
        for (i, (namespace, entries)) in namespaces.iter().enumerate() {
            out.push_str(if i == 0 { "\n" } else { ",\n" });
            out.push_str(&format!("  {}: {{", string(namespace)));
            for (j, (name, listed)) in entries.iter().enumerate() {
                out.push_str(if j == 0 { "\n" } else { ",\n" });
                out.push_str(&format!(
                    "   {}: {{\n    \"updated\": {}",
                    string(name),
                    string(&listed.updated)
                ));
                if let Some(sha256) = &listed.sha256 {
                    out.push_str(&format!(",\n    \"sha256\": {}", string(sha256)));
                }
                out.push_str("\n   }");
            }
            out.push_str("\n  }");
        }
        out.push_str(if namespaces.is_empty() { "}\n}\n" } else { "\n }\n}\n" });
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Dependency {
    name: String,
    text: String,
    /// Why it can't be read, if it can't.
    req: Result<VersionReq, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequiresAnsible {
    text: String,
    spec: Result<SpecifierSet, String>,
}

/// What an index line holds, to build one with `CollectionVersion::new`.
#[derive(Clone, Debug)]
pub struct VersionFields {
    pub name: String,
    pub version: Version,
    /// Each dependency's name and requirement, as `MANIFEST.json` gave them.
    pub deps: Vec<(String, String)>,
    pub sha256: String,
    pub size: u64,
    pub requires_ansible: Option<String>,
    pub yanked: bool,
    pub published: String,
    pub deprecated: bool,
}

/// One line of an index file, one version of a collection.
///
/// A dependency or `requires_ansible` that can't be read leaves the version unusable rather than unreadable, as the
/// line is fine and only its metadata isn't. `problem` says why, and `deps` and `requires_ansible` hold what could be
/// read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionVersion {
    name: String,
    version: Version,
    deps: Vec<Dependency>,
    sha256: String,
    size: u64,
    requires_ansible: Option<RequiresAnsible>,
    yanked: bool,
    published: String,
    deprecated: bool,
}

impl CollectionVersion {
    pub fn new(fields: VersionFields) -> Result<Self, ParseError> {
        check_collection_name(&fields.name)?;
        check_sha256("sha256", &fields.sha256)?;
        check_time("published", &fields.published)?;
        let mut cache = Cache::default();
        Ok(CollectionVersion {
            deps: fields
                .deps
                .into_iter()
                .map(|(name, text)| cache.dependency(name, text))
                .collect(),
            requires_ansible: fields.requires_ansible.map(|text| cache.requires_ansible(text)),
            name: fields.name,
            version: fields.version,
            sha256: fields.sha256,
            size: fields.size,
            yanked: fields.yanked,
            published: fields.published,
            deprecated: fields.deprecated,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    /// Each dependency, in the order listed. An error if any can't be read, which `problem` also says.
    pub fn deps(&self) -> Result<Vec<(&str, &VersionReq)>, String> {
        self.deps
            .iter()
            .map(|d| match &d.req {
                Ok(req) => Ok((d.name.as_str(), req)),
                Err(why) => Err(format!("{} {}: {why}", self.name, self.version)),
            })
            .collect()
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// The ansible-core versions it supports, or None for any. An error if it can't be read.
    pub fn requires_ansible(&self) -> Result<Option<&SpecifierSet>, String> {
        match &self.requires_ansible {
            None => Ok(None),
            Some(RequiresAnsible { spec: Ok(spec), .. }) => Ok(Some(spec)),
            Some(RequiresAnsible { spec: Err(why), .. }) => Err(format!("{} {}: {why}", self.name, self.version)),
        }
    }

    pub fn yanked(&self) -> bool {
        self.yanked
    }

    pub fn published(&self) -> &str {
        &self.published
    }

    pub fn deprecated(&self) -> bool {
        self.deprecated
    }

    /// Why this version can't be used, such as a dependency requirement ansible-galaxy can't read, or None.
    pub fn problem(&self) -> Option<String> {
        let problems: Vec<&str> = self
            .deps
            .iter()
            .filter_map(|d| d.req.as_ref().err())
            .chain(self.requires_ansible.iter().filter_map(|r| r.spec.as_ref().err()))
            .map(String::as_str)
            .collect();
        (!problems.is_empty()).then(|| problems.join("; "))
    }

    /// The line as the index stores it. A line with a `problem` is refused unless `allow_unreadable` is set, which
    /// only a mirror that must publish what authors published should use.
    pub fn to_json(&self, allow_unreadable: bool) -> Result<String, ParseError> {
        if !allow_unreadable && let Some(problem) = self.problem() {
            return Err(ParseError::new(format!("{} {}: {problem}", self.name, self.version)));
        }
        check_collection_name(&self.name)?;
        check_time("published", &self.published)?;
        let deps = self
            .deps
            .iter()
            .map(|d| format!("{{\"name\":{},\"req\":{}}}", string(&d.name), string(&d.text)))
            .collect::<Vec<_>>()
            .join(",");
        let mut out = format!(
            "{{\"v\":{LINE_VERSION},\"name\":{},\"vers\":{},\"deps\":[{deps}],\"sha256\":{},\"size\":{}",
            string(&self.name),
            string(self.version.text()),
            string(&self.sha256),
            self.size,
        );
        if let Some(requires_ansible) = &self.requires_ansible {
            out.push_str(&format!(",\"requires_ansible\":{}", string(&requires_ansible.text)));
        }
        out.push_str(&format!(",\"yanked\":{}", self.yanked));
        if self.deprecated {
            out.push_str(",\"deprecated\":true");
        }
        out.push_str(&format!(",\"published\":{}}}", string(&self.published)));
        Ok(out)
    }
}

/// Requirements parsed once per text. An index repeats a few hundred across thousands of lines.
#[derive(Default)]
struct Cache {
    names: HashMap<String, Result<(), String>>,
    reqs: HashMap<String, Result<VersionReq, String>>,
    specs: HashMap<String, Result<SpecifierSet, String>>,
}

impl Cache {
    fn dependency(&mut self, name: String, text: String) -> Dependency {
        let named = self
            .names
            .entry(name.clone())
            .or_insert_with(|| check_collection_name(&name).map_err(|e| e.to_string()))
            .clone();
        let req = named.and_then(|()| {
            self.reqs
                .entry(text.clone())
                .or_insert_with(|| VersionReq::parse_galaxy(&text).map_err(|e| e.to_string()))
                .clone()
        });
        Dependency {
            req: req.map_err(|e| format!("dependency {name}: {e}")),
            name,
            text,
        }
    }

    fn requires_ansible(&mut self, text: String) -> RequiresAnsible {
        let spec = self
            .specs
            .entry(text.clone())
            .or_insert_with(|| SpecifierSet::parse(&text).map_err(|e| e.to_string()))
            .clone()
            .map_err(|e| format!("requires_ansible '{text}': {e}"));
        RequiresAnsible { text, spec }
    }
}

/// One collection's index file, its lines in the file's order, which is publication order rather than version order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionIndex {
    name: String,
    lines: Vec<CollectionVersion>,
    newer: usize,
    invalid: Vec<(usize, String)>,
}

impl CollectionIndex {
    pub fn new(name: &str, lines: Vec<CollectionVersion>) -> Result<Self, ParseError> {
        check_collection_name(name)?;
        let mut seen = std::collections::HashSet::new();
        for line in &lines {
            if line.name != name {
                return Err(ParseError::new(format!(
                    "{} {} doesn't belong in {name}'s index file",
                    line.name, line.version
                )));
            }
            if !seen.insert(line.version.text()) {
                return Err(ParseError::new(format!(
                    "version {} is listed more than once",
                    line.version
                )));
            }
        }
        Ok(CollectionIndex {
            name: name.to_owned(),
            lines,
            newer: 0,
            invalid: Vec::new(),
        })
    }

    /// Read collection `name`'s index file. A line that can't be read is skipped, with its number and why in
    /// `invalid`, and one whose `v` is newer than this reads is counted in `newer`.
    pub fn parse(data: &[u8], name: &str) -> Self {
        let mut file = CollectionIndex {
            name: name.to_owned(),
            lines: Vec::new(),
            newer: 0,
            invalid: Vec::new(),
        };
        let mut cache = Cache::default();
        let mut seen = std::collections::HashSet::new();
        for (i, line) in data.split(|&b| b == b'\n').enumerate() {
            if is_blank(line) {
                continue;
            }
            let read = object(line).and_then(|obj| {
                if obj.get("v").and_then(Value::as_u64).is_some_and(|v| v > LINE_VERSION) {
                    return Ok(None);
                }
                read_line(&obj, name, &mut cache).map(Some)
            });
            match read {
                Ok(None) => file.newer += 1,
                Ok(Some(line)) if !seen.insert(line.version.text().to_owned()) => {
                    let why = format!("version {} is listed more than once", line.version);
                    file.invalid.push((i + 1, why));
                }
                Ok(Some(line)) => file.lines.push(line),
                Err(why) => file.invalid.push((i + 1, why)),
            }
        }
        file
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn lines(&self) -> &[CollectionVersion] {
        &self.lines
    }

    /// How many lines were skipped because their `v` is newer than this reads.
    pub fn newer(&self) -> usize {
        self.newer
    }

    /// Each line that couldn't be read, by number from 1, and why.
    pub fn invalid(&self) -> &[(usize, String)] {
        &self.invalid
    }

    /// The name, lines, newer count and invalid lines, taken apart rather than copied.
    pub fn into_parts(self) -> (String, Vec<CollectionVersion>, usize, Vec<(usize, String)>) {
        (self.name, self.lines, self.newer, self.invalid)
    }

    /// The file as the index stores it, a line each, as `CollectionVersion::to_json` writes them.
    pub fn to_jsonl(&self, allow_unreadable: bool) -> Result<String, ParseError> {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(&line.to_json(allow_unreadable)?);
            out.push('\n');
        }
        Ok(out)
    }
}

fn read_line(obj: &Map<String, Value>, name: &str, cache: &mut Cache) -> Result<CollectionVersion, String> {
    if obj.get("v").and_then(Value::as_u64) != Some(LINE_VERSION) {
        return Err(format!("'v' must be {LINE_VERSION}"));
    }
    if obj.get("name").and_then(Value::as_str) != Some(name) {
        return Err(format!("'name' must be '{name}', the file's collection"));
    }
    // The version first, so its message wins over any later field's.
    let vers = text(obj, "vers")?;
    let version = Version::parse(vers).map_err(|e| e.to_string())?;
    let sha256 = text(obj, "sha256")?;
    if !is_sha256(sha256) {
        return Err(format!("{vers}: 'sha256' must be a lowercase hex sha256"));
    }
    let size = match obj.get("size") {
        Some(Value::Number(n)) if n.as_i64().is_some_and(|n| n < 0) => {
            return Err(format!("{vers}: 'size' must not be negative"));
        }
        Some(Value::Number(n)) if n.as_u64().is_some() => n.as_u64().expect("checked"),
        _ => return Err(required("size", "an integer")),
    };
    let Some(yanked) = obj.get("yanked").and_then(Value::as_bool) else {
        return Err(required("yanked", "a boolean"));
    };
    let published = text(obj, "published")?;
    let Some(raw_deps) = obj.get("deps").and_then(Value::as_array) else {
        return Err(required("deps", "an array"));
    };
    let deprecated = match obj.get("deprecated") {
        None | Some(Value::Bool(false)) => false,
        // The spec allows a message. Nothing writes one, so it's read as deprecated and the message dropped.
        Some(Value::Bool(true) | Value::String(_)) => true,
        Some(_) => return Err(format!("{vers}: 'deprecated' must be true or a message")),
    };
    let mut deps = Vec::with_capacity(raw_deps.len());
    for dep in raw_deps {
        let field = |key: &str| dep.as_object().and_then(|d| d.get(key)).and_then(Value::as_str);
        let (Some(dep_name), Some(req)) = (field("name"), field("req")) else {
            return Err(format!("{vers}: each of 'deps' must have a 'name' and a 'req' string"));
        };
        deps.push(cache.dependency(dep_name.to_owned(), req.to_owned()));
    }
    let requires_ansible = match obj.get("requires_ansible") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(cache.requires_ansible(text.clone())),
        Some(_) => return Err(format!("{vers}: 'requires_ansible' must be a string")),
    };
    Ok(CollectionVersion {
        name: name.to_owned(),
        version,
        deps,
        sha256: sha256.to_owned(),
        size,
        requires_ansible,
        yanked,
        published: published.to_owned(),
        deprecated,
    })
}

fn required(key: &str, kind: &str) -> String {
    format!("'{key}' is required and must be {kind}")
}

fn text<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| required(key, "a string"))
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_sha256(key: &str, text: &str) -> Result<(), ParseError> {
    if is_sha256(text) {
        Ok(())
    } else {
        Err(ParseError::new(format!(
            "'{key}' must be a lowercase hex sha256, not '{text}'"
        )))
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// An RFC 3339 time in UTC with second precision, such as `2026-10-05T17:30:02Z`, as the spec has them.
fn check_time(key: &str, text: &str) -> Result<(), ParseError> {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    let number = |range: std::ops::Range<usize>| text[range].parse::<u32>().unwrap_or(99);
    let valid = b.len() == 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b'Z'
        && (1..=12).contains(&number(5..7))
        && (1..=days_in_month(number(0..4), number(5..7))).contains(&number(8..10))
        && number(11..13) <= 23
        && number(14..16) <= 59
        && number(17..19) <= 60;
    if valid {
        Ok(())
    } else {
        Err(ParseError::new(format!(
            "'{key}' must be an RFC 3339 time in UTC, such as 2026-10-05T17:30:02Z, not '{text}'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        for valid in [
            "2026-10-05T17:30:02Z",
            "2028-02-29T00:00:00Z",
            "2000-02-29T23:59:59Z",
            "2026-12-31T23:59:60Z",
            "2026-04-30T12:00:00Z",
        ] {
            assert!(check_time("t", valid).is_ok(), "{valid}");
        }
        for invalid in [
            "2026-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-04-31T12:00:00Z",
            "2026-00-10T12:00:00Z",
            "2026-13-10T12:00:00Z",
            "2026-10-00T12:00:00Z",
            "2026-10-05T24:00:00Z",
            "2026-10-05T17:60:02Z",
            "2026-10-05T17:30:61Z",
            "2026-10-05T17:30:02",
            "2026-10-05T17:30:02+00:00",
            "2026-10-05 17:30:02Z",
            "2026-10-05T17:30:02.5Z",
            "26-10-05T17:30:02Z",
            "",
        ] {
            assert!(check_time("t", invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn version_in_a_url() {
        assert_eq!(percent_encode("1.0.0-rc.1"), "1.0.0-rc.1");
        assert_eq!(percent_encode("1.0.0+build.1"), "1.0.0%2Bbuild.1");
        assert_eq!(percent_encode("a_b~c"), "a_b~c");
        assert_eq!(percent_encode("a/b c"), "a%2Fb%20c");
    }
}
