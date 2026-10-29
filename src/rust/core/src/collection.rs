// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! A collection's metadata, as ansible-galaxy reads it from `galaxy.yml` in a source tree or `MANIFEST.json`'s
//! `collection_info` in a built one, and its `meta/runtime.yml`.
//!
//! Reading `galaxy.yml` and the runtime files only fails when the file can't be read at all. A value is checked when
//! it's asked for, so a caller that doesn't need one isn't stopped by it, and the runtime files keep only the routing
//! entries of the expected type. `MANIFEST.json` is what ansible-galaxy writes, in its one format, so one that breaks
//! that is refused, though the metadata in it is still checked only when asked for.

use std::fmt;

use serde_json::Value as Json;

use crate::pep440::SpecifierSet;
use crate::semver::{Version, VersionReq};
use crate::yaml::{Document, Value, YamlError};

pub use crate::names::check_collection_name;

/// Why a value isn't one ansible-galaxy would take, each as the field it's about, or None for the whole file, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataError {
    pub errors: Vec<(Option<String>, String)>,
}

impl MetadataError {
    fn of(field: &str, message: impl Into<String>) -> Self {
        MetadataError {
            errors: vec![(Some(field.to_owned()), message.into())],
        }
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self
            .errors
            .iter()
            .map(|(field, message)| match field {
                Some(field) => format!("{field}: {message}"),
                None => message.clone(),
            })
            .collect();
        f.write_str(&lines.join("\n"))
    }
}

impl std::error::Error for MetadataError {}

/// Why reading metadata failed: the text isn't YAML or JSON, its top level isn't a mapping, or `MANIFEST.json` breaks
/// its format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    Yaml(YamlError),
    Json(String),
    Invalid(MetadataError),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Yaml(err) => err.fmt(f),
            ReadError::Json(message) => f.write_str(message),
            ReadError::Invalid(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ReadError {}

/// What `galaxy.yml` or `MANIFEST.json`'s `collection_info` says about a collection. Each value is as written, and
/// the `get_*` methods check the ones a caller needs by the rules ansible-galaxy installs with.
#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    /// The file, or `collection_info`, whose top level is a mapping or null. Its warnings are the YAML's, such as a
    /// duplicate key.
    pub document: Document,
}

impl Metadata {
    pub fn from_galaxy_yml(data: &[u8]) -> Result<Self, ReadError> {
        let text = crate::yaml::decode(data).map_err(ReadError::Yaml)?;
        Self::of(crate::yaml::loads(text).map_err(ReadError::Yaml)?)
    }

    fn of(document: Document) -> Result<Self, ReadError> {
        match document.resolve(&document.root) {
            Value::Mapping(_) | Value::Null => Ok(Metadata { document }),
            _ => Err(ReadError::Invalid(MetadataError {
                errors: vec![(None, "expected a mapping at the top level".to_owned())],
            })),
        }
    }

    /// The top-level value `key` names, as written, its aliases resolved by `document`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        mapping_get(&self.document, key)
    }

    fn text(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Whether the top level has `key`, even as null.
    pub fn has(&self, key: &str) -> bool {
        match self.document.resolve(&self.document.root) {
            Value::Mapping(entries) => entries
                .iter()
                .any(|(k, _)| matches!(self.document.resolve(k), Value::String(s) if s == key)),
            _ => false,
        }
    }

    /// The collection's full name, `namespace.name`, if both are strings that make a valid one.
    pub fn get_fqcn(&self) -> Result<String, MetadataError> {
        let (Some(namespace), Some(name)) = (self.text("namespace"), self.text("name")) else {
            return Err(MetadataError::of("name", "'namespace' and 'name' are required strings"));
        };
        let fqcn = format!("{namespace}.{name}");
        check_collection_name(&fqcn).map_err(|e| MetadataError::of("name", e.to_string()))?;
        Ok(fqcn)
    }

    /// The version, if it's a SemVer string.
    pub fn get_version(&self) -> Result<Version, MetadataError> {
        let text = self.text("version").ok_or_else(|| {
            MetadataError::of("version", "'version' is required and must be a string, such as '1.0.0'")
        })?;
        Version::parse(text).map_err(|e| MetadataError::of("version", e.to_string()))
    }

    /// Each dependency's name and requirement, if every one is valid. Every problem is reported rather than the first.
    pub fn get_dependencies(&self) -> Result<Vec<(String, VersionReq)>, MetadataError> {
        let deps = match self.get("dependencies") {
            None | Some(Value::Null) => return Ok(Vec::new()),
            Some(Value::Mapping(deps)) => deps,
            Some(_) => {
                let why = "'dependencies' must be a mapping of collection names to versions";
                return Err(MetadataError::of("dependencies", why));
            }
        };
        let fqcn = self.get_fqcn().ok();
        let doc = &self.document;
        let mut dependencies = Vec::new();
        let mut errors = Vec::new();
        for (key, value) in deps {
            let (key, value) = (doc.resolve(key), doc.resolve(value));
            let at = Some(format!("dependencies.{}", shown(key)));
            let (Value::String(name), Value::String(req)) = (key, value) else {
                let why = "expected a collection name and a version requirement string, such as '*'";
                errors.push((at, why.to_owned()));
                continue;
            };
            if let Err(e) = check_collection_name(name) {
                errors.push((at, e.to_string()));
            } else if fqcn.as_deref() == Some(name.as_str()) {
                errors.push((at, "the collection can't depend on itself".to_owned()));
            } else {
                match VersionReq::parse_galaxy(req) {
                    Ok(req) => dependencies.push((name.clone(), req)),
                    Err(e) => errors.push((at, e.to_string())),
                }
            }
        }
        if errors.is_empty() {
            Ok(dependencies)
        } else {
            Err(MetadataError { errors })
        }
    }
}

/// `MANIFEST.json`, as ansible-galaxy builds it, with format 1, the only one there is: the collection's metadata as
/// `collection_info`, and `FILES.json`'s sha256 in `file_manifest_file`.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub collection_info: Metadata,
    /// `FILES.json`'s sha256, in lowercase hex.
    pub files_sha256: String,
}

impl Manifest {
    /// Read `MANIFEST.json`, refusing one ansible-galaxy wouldn't have built. Every problem is reported rather than
    /// the first, and the values in `collection_info` are checked only when asked for, as `Metadata` says.
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let obj = crate::json::object(data).map_err(ReadError::Json)?;
        let mut errors = Vec::new();
        let mut fail = |field: &str, message: &str| errors.push((Some(field.to_owned()), message.to_owned()));
        let format = Json::from(1);
        let collection_info = match obj.get("collection_info") {
            Some(info @ Json::Object(_)) => Some(Metadata {
                document: Document {
                    root: from_json(info),
                    anchors: Vec::new(),
                    warnings: Vec::new(),
                },
            }),
            _ => {
                fail("collection_info", "expected an object of the collection's metadata");
                None
            }
        };
        if obj.get("format") != Some(&format) {
            fail("format", "expected 1, the only format there is");
        }
        let mut files_sha256 = None;
        match obj.get("file_manifest_file") {
            Some(Json::Object(files)) => {
                for (key, expected) in [("name", "FILES.json"), ("ftype", "file"), ("chksum_type", "sha256")] {
                    if files.get(key).and_then(Json::as_str) != Some(expected) {
                        fail(&format!("file_manifest_file.{key}"), &format!("expected '{expected}'"));
                    }
                }
                if files.get("format") != Some(&format) {
                    fail("file_manifest_file.format", "expected 1, the only format there is");
                }
                match files.get("chksum_sha256").and_then(Json::as_str) {
                    Some(hex) if hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => {
                        files_sha256 = Some(hex.to_owned());
                    }
                    _ => fail("file_manifest_file.chksum_sha256", "expected a sha256 in lowercase hex"),
                }
            }
            _ => fail(
                "file_manifest_file",
                "expected an object naming FILES.json and giving its sha256",
            ),
        }
        match (collection_info, files_sha256, errors.is_empty()) {
            (Some(collection_info), Some(files_sha256), true) => Ok(Manifest {
                collection_info,
                files_sha256,
            }),
            _ => Err(ReadError::Invalid(MetadataError { errors })),
        }
    }
}

/// `meta/runtime.yml`, as ansible-core reads it, with PyYAML's `BaseLoader`, so every scalar in it is a string.
#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    /// `requires_ansible` as written, its aliases resolved by `plugin_routing.document`.
    pub requires_ansible: Option<Value>,
    pub plugin_routing: PluginRouting,
    pub warnings: Vec<crate::yaml::Warning>,
}

/// `extensions/eda/eda_runtime.yml`, as ansible-rulebook reads it, for its event sources and filters, from 1.2.1.
/// ansible-rulebook never reads `meta/runtime.yml`. It reads only `event_source` and `event_filter` from this file,
/// but every type is kept, as `Runtime` keeps them, so one it adds needs no change here.
#[derive(Clone, Debug, PartialEq)]
pub struct EdaRuntime {
    pub plugin_routing: PluginRouting,
    pub warnings: Vec<crate::yaml::Warning>,
}

impl EdaRuntime {
    /// Read `eda_runtime.yml`, keeping the entries ansible-rulebook would take. It reads the file with
    /// `yaml.safe_load`, and refuses a whole file whose top level isn't a mapping or has no `plugin_routing` mapping,
    /// which is read as empty here, and an entry that isn't a mapping or has a `deprecation`, `tombstone` or
    /// `redirect` of the wrong type, which is left out. One that's empty or false is taken as not there.
    /// ansible-rulebook loads each value typed, but the values here are the strings written, to match what `Runtime`
    /// gives for `meta/runtime.yml`.
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let text = crate::yaml::decode(data).map_err(ReadError::Yaml)?;
        let typed = crate::yaml::loads(text).map_err(ReadError::Yaml)?;
        let doc = crate::yaml::loads_untyped(text).map_err(ReadError::Yaml)?;
        let types = eda_routing(&typed, &doc);
        Ok(EdaRuntime {
            warnings: doc.warnings.clone(),
            plugin_routing: PluginRouting { document: doc, types },
        })
    }
}

/// `eda_runtime.yml`'s routing, judged on `typed`, as ansible-rulebook's `safe_load` gives it, and taken from the
/// same text read untyped, `doc`.
fn eda_routing(typed: &Document, doc: &Document) -> RoutingTypes {
    let Some(Value::Mapping(by_type)) = mapping_get(typed, "plugin_routing") else {
        return Vec::new();
    };
    let mut types = Vec::new();
    for (kind, entries) in by_type {
        let (Value::String(kind), Value::Mapping(entries)) = (typed.resolve(kind), typed.resolve(entries)) else {
            continue;
        };
        let raw_entries = match mapping_get(doc, "plugin_routing") {
            Some(Value::Mapping(raw_types)) => match field(doc, raw_types, kind) {
                Some(Value::Mapping(raw)) => raw.as_slice(),
                _ => &[],
            },
            _ => &[],
        };
        let mut named = Vec::new();
        for (index, (name, entry)) in entries.iter().enumerate() {
            let name = shown(typed.resolve(name));
            let Value::Mapping(fields) = typed.resolve(entry) else {
                continue;
            };
            let set = |key: &str| field(typed, fields, key).filter(|v| truthy(v));
            let removals_ok = ["deprecation", "tombstone"]
                .iter()
                .all(|key| set(key).is_none_or(|v| matches!(v, Value::Mapping(_))));
            if !removals_ok || set("redirect").is_some_and(|v| !matches!(v, Value::String(_))) {
                continue;
            }
            // The two reads have the same entries in the same order, unless keys typing made equal merged.
            let raw = if raw_entries.len() == entries.len() {
                raw_entries.get(index).map(|(_, v)| v)
            } else {
                raw_entries
                    .iter()
                    .find(|(k, _)| shown(doc.resolve(k)) == name)
                    .map(|(_, v)| v)
            };
            let Some(raw) = raw else {
                continue;
            };
            let Value::Mapping(raw_fields) = doc.resolve(raw) else {
                continue;
            };
            let removal = |key: &str| set(key).and_then(|_| removal(doc, raw_fields, key));
            named.push((
                name,
                RoutingEntry {
                    redirect: set("redirect").and_then(|_| text_field(doc, raw_fields, "redirect")),
                    action_plugin: None,
                    deprecation: removal("deprecation"),
                    tombstone: removal("tombstone"),
                },
                raw.clone(),
            ));
        }
        types.push((kind.clone(), named));
    }
    types
}

/// Whether a value is true to Python, as ansible-rulebook tests a `deprecation`, `tombstone` or `redirect`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Sequence(items) => !items.is_empty(),
        Value::Mapping(entries) => !entries.is_empty(),
        Value::BigInt(_) | Value::Alias(_) => true,
    }
}

/// `plugin_routing`: each plugin type, such as `modules`, with each name's entry, and the entry as written, as the
/// format may add fields `RoutingEntry` doesn't have. The entries as written are values of `document`, whose anchors
/// an alias in one names.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRouting {
    pub document: Document,
    pub types: RoutingTypes,
}

/// Each plugin type, with each of its plugins' names, routing, and entry as written.
pub type RoutingTypes = Vec<(String, Vec<(String, RoutingEntry, Value)>)>;

/// A plugin's routing, with the fields ansible-test's `runtime-metadata` schema gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutingEntry {
    /// The full name of the plugin used in its place.
    pub redirect: Option<String>,
    /// For a module, the action plugin that runs it.
    pub action_plugin: Option<String>,
    pub deprecation: Option<Removal>,
    pub tombstone: Option<Removal>,
}

impl RoutingEntry {
    /// The routing a plugin index line gives, as the index copied it from the collection's runtime file.
    pub fn from_json(entry: &serde_json::Map<String, Json>) -> Self {
        let text =
            |obj: &serde_json::Map<String, Json>, key: &str| obj.get(key).and_then(Json::as_str).map(str::to_owned);
        let removal = |key: &str| {
            entry.get(key).and_then(Json::as_object).map(|obj| Removal {
                removal_version: text(obj, "removal_version"),
                removal_date: text(obj, "removal_date"),
                warning_text: text(obj, "warning_text"),
            })
        };
        RoutingEntry {
            redirect: text(entry, "redirect"),
            action_plugin: text(entry, "action_plugin"),
            deprecation: removal("deprecation"),
            tombstone: removal("tombstone"),
        }
    }
}

/// When a plugin is or was removed, and what to say about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    pub removal_version: Option<String>,
    pub removal_date: Option<String>,
    pub warning_text: Option<String>,
}

impl Runtime {
    /// Read `meta/runtime.yml`. A top level that isn't a mapping, which ansible-core can't load, is read as empty.
    pub fn parse(data: &[u8]) -> Result<Self, ReadError> {
        let text = crate::yaml::decode(data).map_err(ReadError::Yaml)?;
        let doc = crate::yaml::loads_untyped(text).map_err(ReadError::Yaml)?;
        Ok(Runtime {
            requires_ansible: mapping_get(&doc, "requires_ansible").cloned(),
            warnings: doc.warnings.clone(),
            plugin_routing: PluginRouting::of(doc),
        })
    }

    /// `requires_ansible` as a PEP 440 specifier set, or None if it isn't set. Anything else is an error.
    pub fn get_requires_ansible(&self) -> Result<Option<SpecifierSet>, MetadataError> {
        let field = "requires_ansible";
        match &self.requires_ansible {
            None => Ok(None),
            Some(Value::String(text)) => SpecifierSet::parse(text)
                .map(Some)
                .map_err(|e| MetadataError::of(field, format!("{e}, expected PEP 440 such as '>=2.17'"))),
            Some(_) => Err(MetadataError::of(field, "'requires_ansible' must be a string")),
        }
    }
}

impl PluginRouting {
    /// Each type's entries that are mappings, by name.
    fn of(doc: Document) -> Self {
        let mut types = Vec::new();
        if let Some(Value::Mapping(by_type)) = mapping_get(&doc, "plugin_routing") {
            for (kind, entries) in by_type {
                let (Value::String(kind), Value::Mapping(entries)) = (doc.resolve(kind), doc.resolve(entries)) else {
                    continue;
                };
                let named = entries
                    .iter()
                    .filter_map(|(name, entry)| {
                        let Value::Mapping(fields) = doc.resolve(entry) else {
                            return None;
                        };
                        let routing = RoutingEntry {
                            redirect: text_field(&doc, fields, "redirect"),
                            action_plugin: text_field(&doc, fields, "action_plugin"),
                            deprecation: removal(&doc, fields, "deprecation"),
                            tombstone: removal(&doc, fields, "tombstone"),
                        };
                        Some((shown(doc.resolve(name)), routing, entry.clone()))
                    })
                    .collect();
                types.push((kind.clone(), named));
            }
        }
        PluginRouting { document: doc, types }
    }
}

fn field<'a>(doc: &'a Document, fields: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find(|(k, _)| matches!(doc.resolve(k), Value::String(s) if s == key))
        .map(|(_, v)| doc.resolve(v))
}

fn text_field(doc: &Document, fields: &[(Value, Value)], key: &str) -> Option<String> {
    match field(doc, fields, key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn removal(doc: &Document, fields: &[(Value, Value)], key: &str) -> Option<Removal> {
    let Some(Value::Mapping(removal)) = field(doc, fields, key) else {
        return None;
    };
    Some(Removal {
        removal_version: text_field(doc, removal, "removal_version"),
        removal_date: text_field(doc, removal, "removal_date"),
        warning_text: text_field(doc, removal, "warning_text"),
    })
}

fn mapping_get<'a>(doc: &'a Document, key: &str) -> Option<&'a Value> {
    let Value::Mapping(entries) = doc.resolve(&doc.root) else {
        return None;
    };
    field(doc, entries, key)
}

/// A key as a message, or a name, shows it: a string as it is, anything else as YAML wrote it.
fn shown(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(digits) => digits.clone(),
        Value::Float(f) => f.to_string(),
        _ => "?".to_owned(),
    }
}

fn from_json(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => n.as_f64().map_or_else(|| Value::BigInt(n.to_string()), Value::Float),
        },
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::Sequence(items.iter().map(from_json).collect()),
        Json::Object(obj) => Value::Mapping(
            obj.iter()
                .map(|(k, v)| (Value::String(k.clone()), from_json(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn galaxy(text: &str) -> Metadata {
        Metadata::from_galaxy_yml(text.as_bytes()).unwrap()
    }

    fn fields(err: MetadataError) -> Vec<String> {
        err.errors.into_iter().map(|(f, _)| f.unwrap()).collect()
    }

    #[test]
    fn getters() {
        let m = galaxy("namespace: ns\nname: coll\nversion: 1.0.0\ndependencies: {a.b: '>=1'}\n");
        assert_eq!(m.get_fqcn().unwrap(), "ns.coll");
        assert_eq!(m.get_version().unwrap().text(), "1.0.0");
        let deps = m.get_dependencies().unwrap();
        assert_eq!((deps[0].0.as_str(), deps[0].1.text()), ("a.b", ">=1"));
        assert!(galaxy("").get_dependencies().unwrap().is_empty());
    }

    #[test]
    fn read_without_checking() {
        // Each value is kept as written and checked only when asked for.
        let m = galaxy(
            "namespace: ns\nname: coll\nversion: 1.0\ndependencies: {Bad.x: '*', a.b: '^1', ns.coll: '*', c.d: 1}\n",
        );
        assert_eq!(m.get("version"), Some(&Value::Float(1.0)));
        assert_eq!(fields(m.get_version().unwrap_err()), ["version"]);
        assert_eq!(
            fields(m.get_dependencies().unwrap_err()),
            [
                "dependencies.Bad.x",
                "dependencies.a.b",
                "dependencies.ns.coll",
                "dependencies.c.d"
            ]
        );
        assert_eq!(fields(galaxy("name: coll\n").get_fqcn().unwrap_err()), ["name"]);
        assert_eq!(
            fields(galaxy("namespace: ns\nname: Coll\n").get_fqcn().unwrap_err()),
            ["name"]
        );
        assert_eq!(
            fields(galaxy("dependencies: [a]\n").get_dependencies().unwrap_err()),
            ["dependencies"]
        );
        assert!(matches!(
            Metadata::from_galaxy_yml(b"- a\n"),
            Err(ReadError::Invalid(_))
        ));
    }

    const SHA256: &str = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";

    fn manifest_json(info: &str) -> String {
        format!(
            r#"{{"collection_info": {info}, "file_manifest_file": {{"name": "FILES.json", "ftype": "file", "chksum_type": "sha256", "chksum_sha256": "{SHA256}", "format": 1}}, "format": 1}}"#
        )
    }

    #[test]
    fn manifest() {
        let data = manifest_json(r#"{"namespace": "ns", "name": "coll", "version": "1.0.0", "foo": 1}"#);
        let manifest = Manifest::parse(data.as_bytes()).unwrap();
        assert_eq!(manifest.files_sha256, SHA256);
        let m = manifest.collection_info;
        assert_eq!(m.get_version().unwrap().text(), "1.0.0");
        assert_eq!(m.get("foo"), Some(&Value::Int(1)));
        // Its values are checked only when asked for.
        let empty = Manifest::parse(manifest_json("{}").as_bytes()).unwrap().collection_info;
        assert_eq!(fields(empty.get_fqcn().unwrap_err()), ["name"]);
    }

    #[test]
    fn manifest_refused() {
        let Err(ReadError::Invalid(e)) = Manifest::parse(b"{}") else {
            panic!()
        };
        assert_eq!(fields(e), ["collection_info", "format", "file_manifest_file"]);
        let data = manifest_json("{}")
            .replace("FILES.json", "OTHER.json")
            .replace(r#""ftype": "file""#, r#""ftype": "dir""#)
            .replace(SHA256, &SHA256.to_uppercase())
            .replace(r#""format": 1}, "format": 1"#, r#""format": 2}, "format": 2"#);
        let Err(ReadError::Invalid(e)) = Manifest::parse(data.as_bytes()) else {
            panic!()
        };
        assert_eq!(
            fields(e),
            [
                "format",
                "file_manifest_file.name",
                "file_manifest_file.ftype",
                "file_manifest_file.format",
                "file_manifest_file.chksum_sha256"
            ]
        );
    }

    #[test]
    fn runtime() {
        let text = "requires_ansible: '>=2.17'\nplugin_routing:\n  modules:\n    \
                    old: {redirect: ns.c.new, extra: 1}\n    1: {tombstone: {removal_version: 2.0}}\n    bad: x\n";
        let r = Runtime::parse(text.as_bytes()).unwrap();
        assert_eq!(r.get_requires_ansible().unwrap().unwrap().to_string(), ">=2.17");
        let (kind, entries) = &r.plugin_routing.types[0];
        let names: Vec<_> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!((kind.as_str(), names), ("modules", vec!["old", "1"]));
        assert_eq!(entries[0].1.redirect.as_deref(), Some("ns.c.new"));
        // Every scalar is a string, as ansible-core reads the file.
        let tombstone = entries[1].1.tombstone.as_ref().unwrap();
        assert_eq!(tombstone.removal_version.as_deref(), Some("2.0"));
    }

    #[test]
    fn runtime_requires_ansible() {
        assert!(Runtime::parse(b"").unwrap().get_requires_ansible().unwrap().is_none());
        // 2.15 is the string "2.15" as ansible-core reads it, which isn't a specifier set.
        let r = Runtime::parse(b"requires_ansible: 2.15\n").unwrap();
        assert_eq!(r.requires_ansible, Some(Value::String("2.15".to_owned())));
        assert_eq!(fields(r.get_requires_ansible().unwrap_err()), ["requires_ansible"]);
        let e = Runtime::parse(b"requires_ansible: [a]\n")
            .unwrap()
            .get_requires_ansible()
            .unwrap_err();
        assert!(e.to_string().ends_with("'requires_ansible' must be a string"), "{e}");
    }

    #[test]
    fn runtime_top_level() {
        for text in ["", "null\n", "- a\n"] {
            assert!(
                Runtime::parse(text.as_bytes()).unwrap().plugin_routing.types.is_empty(),
                "{text}"
            );
        }
    }

    fn eda(text: &str) -> EdaRuntime {
        EdaRuntime::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn eda_runtime() {
        let text = "plugin_routing:\n  event_source:\n    \
                    old: {redirect: ns.c.new, deprecation: {removal_version: 3.0}}\n    \
                    gone: {tombstone: {warning_text: bye}, deprecation: null}\n  modules:\n    x: {redirect: a.b.c}\n";
        let r = eda(text);
        // Every type is kept, though ansible-rulebook reads only event sources and filters.
        let types: Vec<_> = r.plugin_routing.types.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(types, ["event_source", "modules"]);
        let entries = &r.plugin_routing.types[0].1;
        assert_eq!(entries[0].1.redirect.as_deref(), Some("ns.c.new"));
        // As strings, as Runtime gives them, though ansible-rulebook reads 3.0 as a float.
        let deprecation = entries[0].1.deprecation.as_ref().unwrap();
        assert_eq!(deprecation.removal_version.as_deref(), Some("3.0"));
        // A null deprecation is taken as not there.
        assert!(entries[1].1.deprecation.is_none());
        assert!(entries[1].1.tombstone.is_some());
    }

    #[test]
    fn eda_runtime_left_out() {
        // What ansible-rulebook refuses is left out, and the rest kept.
        for text in ["", "- a\n", "other: 1\n", "plugin_routing: {event_source: [a]}\n"] {
            assert!(
                eda(text).plugin_routing.types.iter().all(|(_, e)| e.is_empty()),
                "{text}"
            );
        }
        let text = "plugin_routing: {event_source: {a: {redirect: 1}, b: {redirect: ns.c.b}, c: null, \
                    d: {deprecation: soon}}}\n";
        let names: Vec<_> = eda(text).plugin_routing.types[0]
            .1
            .iter()
            .map(|(n, _, _)| n.clone())
            .collect();
        assert_eq!(names, ["b"]);
    }
}
