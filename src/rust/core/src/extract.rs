// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! Static extraction of a collection's plugins: the modules, plugins, filters, tests, roles, playbooks and
//! Event-Driven Ansible content it provides, their `short_description` and, optionally, their docs, with its plugin
//! routing.
//!
//! Collections are untrusted, so nothing is imported or run. Names come from file paths, and for filters and tests
//! from the Python syntax tree. Docs come from string constants (`DOCUMENTATION`, `EXAMPLES` and `RETURN`) or a YAML
//! sidecar. A directory is read, never a tarball: one is extracted first, by `artifact::extract`, which keeps the
//! rules for reading one in one place.

mod fragments;
mod python;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ruff_python_ast::ModModule;

pub use fragments::{DocFragment, FragmentIndex, apply_section, full_name};

use crate::collection::{EdaRuntime, Manifest, Metadata, ReadError, Runtime};
use crate::yaml::{Document, Value};

/// `plugins/<dir>` to plugin type, for the directories playbooks name plugins from. Others, such as `module_utils`,
/// `doc_fragments`, `plugin_utils` and `terminal`, aren't named in playbooks.
const PLUGIN_DIRS: [(&str, &str); 12] = [
    ("become", "become"),
    ("cache", "cache"),
    ("callback", "callback"),
    ("cliconf", "cliconf"),
    ("connection", "connection"),
    ("httpapi", "httpapi"),
    ("inventory", "inventory"),
    ("lookup", "lookup"),
    ("netconf", "netconf"),
    ("shell", "shell"),
    ("strategy", "strategy"),
    ("vars", "vars"),
];

/// Event-Driven Ansible content, as (directory, type, extension), in the order ansible-rulebook searches
/// (`ansible_rulebook/collection.py`). The first directory holding a name is the one it loads. Only files directly
/// inside can be named by FQCN, and rulebooks must be `.yml`.
const EDA_DIRS: [(&str, &str, &str); 8] = [
    ("extensions/eda/plugins/event_source/", "event_source", ".py"),
    ("extensions/eda/plugins/event_sources/", "event_source", ".py"),
    ("plugins/event_source/", "event_source", ".py"),
    ("extensions/eda/plugins/event_filter/", "event_filter", ".py"),
    ("extensions/eda/plugins/event_filters/", "event_filter", ".py"),
    ("plugins/event_filter/", "event_filter", ".py"),
    ("extensions/eda/rulebooks/", "rulebook", ".yml"),
    ("rulebooks/", "rulebook", ".yml"),
];

/// The `eda_runtime.yml` routing types ansible-rulebook reads.
const EDA_ROUTING_TYPES: [&str; 2] = ["event_source", "event_filter"];

/// The largest a value kept from YAML may be. Aliases let a few hundred bytes of YAML expand to millions of values
/// written out as JSON, so a value past either is dropped. Real docs stay far below them.
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;

/// The largest file read.
const MAX_FILE: u64 = 20 * 1024 * 1024;

/// What extraction found in a collection.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    /// From `MANIFEST.json`, or `galaxy.yml` in a source checkout. None if it isn't a string.
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    /// Each dependency's name and requirement, where both are strings.
    pub dependencies: Vec<(String, String)>,
    /// From `meta/runtime.yml`, if it's a string.
    pub requires_ansible: Option<String>,
    /// Sorted by type and name.
    pub plugins: Vec<Plugin>,
    /// `plugin_routing` entries for plugin types, from `meta/runtime.yml`, and for event sources and filters from
    /// `extensions/eda/eda_runtime.yml`. Sorted by type and name.
    pub routing: Vec<Routing>,
    /// The doc fragments the collection defines, sorted by name. Read with docs only.
    pub fragments: Vec<DocFragment>,
    /// `plugin_routing.doc_fragments` entries, sorted by name. Read with docs only.
    pub fragment_routing: Vec<(String, Value)>,
    pub issues: Vec<Issue>,
}

/// A module, plugin, filter, test, role, playbook, or Event-Driven Ansible content.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// The `ansible-doc -t` name, such as `module`, `lookup` or `filter`, `role`, `playbook`, or an Event-Driven
    /// Ansible type: `event_source`, `event_filter` or `rulebook`.
    pub kind: String,
    /// Its short name. A subdirectory adds a prefix: `plugins/modules/net/ping.py` is `net.ping`.
    pub name: String,
    /// Whitespace collapsed to single spaces.
    pub short_description: Option<String>,
    /// With docs, if it has any.
    pub docs: Option<PluginDocs>,
}

/// A plugin's docs as the collection wrote them, with no fragments applied. `extends_documentation_fragment` is taken
/// out of `DOCUMENTATION` and `RETURN` and listed in `fragments` and `return_fragments`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginDocs {
    /// `DOCUMENTATION`, a mapping.
    pub documentation: Option<Value>,
    /// `EXAMPLES`, the source text.
    pub examples: Option<String>,
    /// `RETURN`, a mapping.
    pub returns: Option<Value>,
    /// A role's `meta/argument_specs.yml`, by entry point: `main` is `tasks/main.yml`, any other a task file run with
    /// `tasks_from`.
    pub entry_points: Option<Value>,
    pub fragments: Vec<String>,
    pub return_fragments: Vec<String>,
    /// The file within the collection the docs were read from, which may not be the plugin's own: a filter's can be
    /// the file providing it, documenting another of its filters, as ansible-doc shows it. Its `DOCUMENTATION`'s
    /// `name` says which. None for docs made elsewhere.
    pub path: Option<String>,
    /// The sections that can't be read, which are None above, or the whole file. ansible-doc shows no docs at all
    /// for such a plugin; these are kept so the rest can be shown, with what's wrong.
    pub errors: Vec<DocsError>,
    /// The plugin's collection, `ns.coll`, if it's known, to label its versions and dates with.
    pub collection: Option<String>,
    /// Whether the plugin is a module, whose deprecations are shaped differently.
    pub module: bool,
}

/// A section of a plugin's docs that can't be read, or the whole file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocsError {
    /// `DOCUMENTATION`, `EXAMPLES` or `RETURN`, or None for the whole file.
    pub section: Option<&'static str>,
    /// Why, such as the YAML error.
    pub message: String,
    /// The text as written, if it's a string.
    pub text: Option<String>,
}

impl PluginDocs {
    /// These docs with every fragment `lookup` can supply applied, merged as ansible-core does. Names it can't
    /// supply stay in `fragments` and `return_fragments`, so the result can be applied again once more are
    /// available. With `collection`, each version and date is labelled with the collection it belongs to, as
    /// ansible-doc does.
    pub fn apply_fragments<E>(
        &self,
        lookup: &mut impl FnMut(&str) -> Result<Option<DocFragment>, E>,
    ) -> Result<PluginDocs, E> {
        let tag = self.collection.as_deref().map(|c| (c, self.module));
        let (documentation, fragments) = apply_section(
            self.documentation.as_ref(),
            &self.fragments,
            "DOCUMENTATION",
            lookup,
            tag,
        )?;
        let (returns, return_fragments) =
            apply_section(self.returns.as_ref(), &self.return_fragments, "RETURN", lookup, tag)?;
        Ok(PluginDocs {
            documentation,
            returns,
            fragments,
            return_fragments,
            ..self.clone()
        })
    }
}

/// A `plugin_routing` entry, which may name a plugin the collection no longer has.
#[derive(Clone, Debug, PartialEq)]
pub struct Routing {
    pub kind: String,
    pub name: String,
    /// The entry as written: `redirect`, `deprecation`, `tombstone` and anything else.
    pub entry: Value,
}

/// Something in the collection extraction couldn't read fully.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// `python_unparseable` (often Python 2), `filter_test_unresolved` (filter or test names that can't be found
    /// statically), `fragment_unresolved` (a doc fragment built at runtime), or `value_too_large` (a YAML value
    /// past the limits, dropped).
    pub kind: &'static str,
    /// The file within the collection.
    pub path: String,
    /// Such as which value was too large.
    pub detail: String,
}

/// A collection directory's files, read only from inside it.
struct Source {
    root: PathBuf,
    /// Every file, as a `/` path from the root, sorted. A link to a file is one, a link to a directory isn't walked.
    files: Vec<String>,
}

impl Source {
    fn new(root: &Path) -> io::Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} isn't a directory", root.display()),
            ));
        }
        let mut files = Vec::new();
        walk(&root, "", &mut files)?;
        files.sort();
        Ok(Source { root, files })
    }

    /// Whether `path` is one of the collection's files.
    fn has(&self, path: &str) -> bool {
        self.files.binary_search_by(|f| f.as_str().cmp(path)).is_ok()
    }

    /// The file's content, or None if it doesn't exist, isn't a file, leads outside the collection or is too large.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let real = self.root.join(path).canonicalize().ok()?;
        let meta = fs::metadata(&real).ok()?;
        if !real.starts_with(&self.root) || !meta.is_file() || meta.len() > MAX_FILE {
            return None;
        }
        fs::read(real).ok()
    }
}

fn walk(dir: &Path, prefix: &str, files: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        // A name that isn't UTF-8 can't be a plugin's.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = format!("{prefix}{name}");
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), files)?;
        } else if kind.is_file() || (kind.is_symlink() && entry.path().is_file()) {
            files.push(path);
        }
    }
    Ok(())
}

#[derive(Default)]
struct Issues(Vec<Issue>);

impl Issues {
    fn add(&mut self, kind: &'static str, path: &str, detail: &str) {
        self.0.push(Issue {
            kind,
            path: path.to_owned(),
            detail: detail.to_owned(),
        });
    }
}

/// A key as Python's `json.dumps` writes it, for YAML keys that aren't strings.
fn json_key(key: &Value) -> String {
    match key {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(digits) => digits.clone(),
        Value::Float(f) => float_text(*f),
        _ => String::new(),
    }
}

fn float_text(f: f64) -> String {
    if f.is_nan() {
        "nan".to_owned()
    } else if f.is_infinite() {
        if f > 0.0 { "inf" } else { "-inf" }.to_owned()
    } else {
        f.to_string()
    }
}

/// `value` with its aliases expanded and its keys made strings, so any JSON writer can write it; None past
/// `MAX_NODES` values or `MAX_DEPTH` levels. A float that isn't finite becomes its text, as JSON has none. A key
/// given twice keeps its first place and its last value, as a Python dict does.
fn json_value(doc: &Document, value: &Value) -> Option<Value> {
    fn copy(doc: &Document, value: &Value, depth: usize, nodes: &mut usize) -> Option<Value> {
        *nodes += 1;
        if *nodes > MAX_NODES || depth > MAX_DEPTH {
            return None;
        }
        Some(match doc.resolve(value) {
            Value::Mapping(entries) => {
                let mut out: Vec<(Value, Value)> = Vec::with_capacity(entries.len());
                let mut index: HashMap<String, usize> = HashMap::new();
                for (key, item) in entries {
                    let key = json_key(doc.resolve(key));
                    let item = copy(doc, item, depth + 1, nodes)?;
                    match index.get(&key) {
                        Some(&i) => out[i].1 = item,
                        None => {
                            index.insert(key.clone(), out.len());
                            out.push((Value::String(key), item));
                        }
                    }
                }
                Value::Mapping(out)
            }
            Value::Sequence(items) => Value::Sequence(
                items
                    .iter()
                    .map(|item| copy(doc, item, depth + 1, nodes))
                    .collect::<Option<_>>()?,
            ),
            Value::Float(f) if !f.is_finite() => Value::String(float_text(*f)),
            other => other.clone(),
        })
    }
    copy(doc, value, 0, &mut 0)
}

/// YAML from a collection, or None if it isn't YAML Ansible could read.
fn yaml(data: &[u8]) -> Option<Document> {
    crate::yaml::loads(crate::yaml::decode(data).ok()?).ok()
}

/// A value read from YAML, kept only while it's in the document it came from.
struct Raw<'d> {
    doc: &'d Document,
    value: &'d Value,
}

impl<'d> Raw<'d> {
    fn resolved(&self) -> &'d Value {
        self.doc.resolve(self.value)
    }

    fn get(&self, key: &str) -> Option<Raw<'d>> {
        match self.resolved() {
            Value::Mapping(entries) => entries
                .iter()
                .find(|(k, _)| matches!(self.doc.resolve(k), Value::String(s) if s == key))
                .map(|(_, v)| Raw {
                    doc: self.doc,
                    value: v,
                }),
            _ => None,
        }
    }

    fn is_mapping(&self) -> bool {
        matches!(self.resolved(), Value::Mapping(_))
    }

    fn text(&self) -> Option<&'d str> {
        match self.resolved() {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// `raw` as a JSON-safe mapping, or None if it isn't one, or is too large, which is an issue.
fn json_mapping(raw: Option<&Raw<'_>>, path: &str, detail: &str, issues: &mut Issues) -> Option<Value> {
    let raw = raw?;
    if !raw.is_mapping() {
        return None;
    }
    let value = json_value(raw.doc, raw.value);
    if value.is_none() {
        issues.add("value_too_large", path, detail);
    }
    value
}

/// Docs as read from one file, before any checking.
struct Docs {
    path: String,
    /// The YAML documents the values below are in.
    documents: Vec<Document>,
    doc: Option<(usize, Value)>,
    examples: Option<String>,
    returns: Option<(usize, Value)>,
    entry_points: Option<(usize, Value)>,
    /// The sections that can't be read, or the whole file.
    errors: Vec<DocsError>,
}

impl Docs {
    fn new(path: &str) -> Self {
        Docs {
            path: path.to_owned(),
            documents: Vec::new(),
            doc: None,
            examples: None,
            returns: None,
            entry_points: None,
            errors: Vec::new(),
        }
    }

    /// Whether ansible-core would take this file as having `DOCUMENTATION`: it's set, or can't be read, which fails
    /// ansible-core rather than letting it look further.
    fn has_documentation(&self, truthy_only: bool) -> bool {
        let unreadable = self
            .errors
            .iter()
            .any(|e| matches!(e.section, None | Some("DOCUMENTATION")));
        unreadable || self.documentation().is_some_and(|d| !truthy_only || truthy(d))
    }

    /// Set `section`'s error, replacing what it had, as a later assignment replaces an earlier one.
    fn set_error(&mut self, section: &'static str, message: String, text: Option<String>) {
        self.errors.retain(|e| e.section != Some(section));
        self.errors.push(DocsError {
            section: Some(section),
            message,
            text,
        });
    }

    fn raw<'s>(&'s self, at: &'s Option<(usize, Value)>) -> Option<Raw<'s>> {
        at.as_ref().map(|(i, value)| Raw {
            doc: &self.documents[*i],
            value,
        })
    }

    /// Keep `document`'s root, or its value at `key`.
    fn keep(&mut self, document: Document, key: Option<&str>) -> Option<(usize, Value)> {
        let value = match key {
            None => document.root.clone(),
            Some(key) => {
                let raw = Raw {
                    doc: &document,
                    value: &document.root,
                };
                raw.get(key)?.value.clone()
            }
        };
        self.documents.push(document);
        Some((self.documents.len() - 1, value))
    }

    /// `DOCUMENTATION`, resolved, if it's set.
    fn documentation(&self) -> Option<&Value> {
        self.raw(&self.doc)
            .map(|raw| raw.resolved())
            .filter(|v| !matches!(v, Value::Null))
    }

    /// `short_description` of the docs, or of a role's `main` entry point. If `DOCUMENTATION`, or the whole file,
    /// can't be read, it's read from the text as far as it, as the YAML may only break further on, or failing that
    /// found in the text as `ansible-doc -l` finds it.
    fn short_description(&self) -> Option<String> {
        for error in &self.errors {
            let found = match (error.section, &error.text) {
                (Some("DOCUMENTATION"), Some(text)) => {
                    crate::yaml::scalar_at(text, &["short_description"]).or_else(|| docstub(text, true))
                }
                (None, Some(text)) if self.path.ends_with(".py") => docstub(text, false),
                (None, Some(text)) => crate::yaml::scalar_at(text, &["DOCUMENTATION", "short_description"])
                    .or_else(|| docstub(text, false)),
                _ => continue,
            };
            let collapsed = found?.split_whitespace().collect::<Vec<_>>().join(" ");
            return (!collapsed.is_empty()).then_some(collapsed);
        }
        // A role's main entry point, if it has any, or else the docs.
        let doc = match self.raw(&self.entry_points) {
            Some(entry_points) if matches!(entry_points.resolved(), Value::Mapping(e) if !e.is_empty()) => {
                entry_points.get("main")
            }
            _ => self.raw(&self.doc),
        }?;
        if !doc.is_mapping() {
            return None;
        }
        let text = doc.get("short_description")?.text()?;
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!collapsed.is_empty()).then_some(collapsed)
    }

    fn plugin_docs(&self, issues: &mut Issues) -> Option<PluginDocs> {
        let path = Some(self.path.clone());
        let mut doc = json_mapping(self.raw(&self.doc).as_ref(), &self.path, "DOCUMENTATION", issues);
        let mut returns = json_mapping(self.raw(&self.returns).as_ref(), &self.path, "RETURN", issues);
        let entry_points = json_mapping(
            self.raw(&self.entry_points).as_ref(),
            &self.path,
            "argument_specs",
            issues,
        );
        let examples = self.examples.clone().filter(|e| !e.trim().is_empty());
        let errors = self.errors.clone();
        if doc.is_none() && returns.is_none() && entry_points.is_none() && examples.is_none() && errors.is_empty() {
            return None;
        }
        Some(PluginDocs {
            fragments: fragment_names(doc.as_mut()),
            return_fragments: fragment_names(returns.as_mut()),
            documentation: doc,
            examples,
            returns,
            entry_points,
            path,
            errors,
            collection: None,
            module: false,
        })
    }
}

/// The `short_description` in `text` as `ansible-doc -l` finds it, with `read_docstub`: its line, and those after it
/// indented further, read as YAML on their own, so the rest needn't be readable. Unlike `read_docstub`, only a line
/// at the docs' top level counts, so an option named `short_description` before it isn't taken for it.
/// `in_documentation` is for text that's already `DOCUMENTATION`'s, where a whole file has to reach a line starting
/// `DOCUMENTATION` first. A final `.` is kept, which `ansible-doc -l` drops, as it is for a description read from
/// the docs.
fn docstub(text: &str, in_documentation: bool) -> Option<String> {
    let mut in_documentation = in_documentation;
    let mut top: Option<usize> = None;
    let mut indent: Option<String> = None;
    let mut stub = String::new();
    for line in text.split_inclusive('\n') {
        let content = line.trim_start();
        let depth = line.len() - content.len();
        if in_documentation {
            if let Some(indent) = &indent {
                if !line.starts_with(indent.as_str()) {
                    break;
                }
                stub.push_str(line);
                continue;
            }
            // The docs' top level is the indentation of their first line.
            let blank = content.trim().is_empty() || content.starts_with('#') || content.starts_with("---");
            if top.is_none() && !blank {
                top = Some(depth);
            }
            if content.starts_with("short_description:") && top == Some(depth) {
                indent = Some(" ".repeat(depth + 1));
                stub.push_str(line);
            }
        } else if line.starts_with("DOCUMENTATION") && (line.contains('=') || line.contains(':')) {
            in_documentation = true;
        }
    }
    let document = crate::yaml::loads(stub.trim()).ok()?;
    let root = Raw {
        doc: &document,
        value: &document.root,
    };
    root.get("short_description")?.text().map(str::to_owned)
}

/// Remove and return `extends_documentation_fragment`, a list or a comma-separated string.
fn fragment_names(doc: Option<&mut Value>) -> Vec<String> {
    let Some(Value::Mapping(entries)) = doc else {
        return Vec::new();
    };
    let Some(index) = entries
        .iter()
        .position(|(k, _)| matches!(k, Value::String(s) if s == "extends_documentation_fragment"))
    else {
        return Vec::new();
    };
    let names: Vec<String> = match entries.remove(index).1 {
        Value::String(s) => s.split(',').map(str::to_owned).collect(),
        Value::Sequence(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    names
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether a value is true to Python, as ansible-core tests a file's `DOCUMENTATION` with `if not docs[0]`.
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

/// The extensions the module loader skips when it looks a module up by name (`MODULE_IGNORE_EXTS`, which is
/// `REJECT_EXTS` with `.yaml`, `.yml` and `.ini`).
const MODULE_IGNORE_EXTS: [&str; 12] = [
    ".pyc", ".pyo", ".swp", ".bak", "~", ".rpm", ".md", ".txt", ".rst", ".yaml", ".yml", ".ini",
];

/// The extensions a file holding docs can have (`DOC_EXTENSIONS`), in the order ansible-core looks for them beside a
/// plugin.
const DOC_EXTENSIONS: [&str; 3] = [".py", ".yml", ".yaml"];

/// Reads the collection's files for docs, as ansible-core's `get_plugin_docs` finds and reads them.
struct Reader<'s> {
    source: &'s Source,
    issues: Issues,
    /// Each Python file parsed, or None if it isn't Python 3, an issue once.
    trees: HashMap<String, Option<Rc<ModModule>>>,
}

impl<'s> Reader<'s> {
    /// The file parsed as Python. A missing or unreadable file parses as an empty module.
    fn tree(&mut self, path: &str) -> Option<Rc<ModModule>> {
        if let Some(tree) = self.trees.get(path) {
            return tree.clone();
        }
        let tree = python::parse(&self.source.read(path).unwrap_or_default()).map(Rc::new);
        if tree.is_none() {
            self.issues.add("python_unparseable", path, "");
        }
        self.trees.insert(path.to_owned(), tree.clone());
        tree
    }

    /// The docs in one file, as ansible-core's `read_docstring` reads them: a Python file's constants, or a YAML
    /// sidecar's `DOCUMENTATION`, `EXAMPLES` and `RETURN`. What ansible-core fails to read is an error, as `Docs` has
    /// it.
    fn file_docs(&mut self, path: &str) -> Docs {
        if path.ends_with(".py") {
            self.python_docs(path)
        } else {
            sidecar_docs(self.source, path)
        }
    }

    /// A Python file's docs, from `python::doc_assignments`, a later assignment replacing an earlier one. Each
    /// section is read on its own, so one that can't be read, which fails ansible-core's `get_docstring`, is an
    /// error with its text, and the rest are kept. A file that isn't Python 3 is an error of the whole file.
    fn python_docs(&mut self, path: &str) -> Docs {
        let mut docs = Docs::new(path);
        let Some(tree) = self.tree(path) else {
            let text = self
                .source
                .read(path)
                .map(|data| String::from_utf8_lossy(&data).into_owned());
            docs.errors.push(DocsError {
                section: None,
                message: "isn't Python 3".to_owned(),
                text,
            });
            return docs;
        };
        for (key, value) in python::doc_assignments(&tree) {
            let document = match value {
                None => {
                    let why = "isn't a string or a dict of literals, which ansible-core can't read".to_owned();
                    docs.set_error(key, why, None);
                    match key {
                        "DOCUMENTATION" => docs.doc = None,
                        "RETURN" => docs.returns = None,
                        _ => docs.examples = None,
                    }
                    continue;
                }
                Some(python::DocValue::Text(text)) if key == "EXAMPLES" => {
                    docs.errors.retain(|e| e.section != Some(key));
                    docs.examples = Some(text);
                    continue;
                }
                Some(python::DocValue::Literal(_)) if key == "EXAMPLES" => {
                    // A dict, which isn't examples' text.
                    docs.errors.retain(|e| e.section != Some(key));
                    docs.examples = None;
                    continue;
                }
                Some(python::DocValue::Text(text)) => match yaml_document(&text) {
                    Ok(document) => document,
                    Err(why) => {
                        docs.set_error(key, why, Some(text));
                        if key == "DOCUMENTATION" {
                            docs.doc = None;
                        } else {
                            docs.returns = None;
                        }
                        continue;
                    }
                },
                Some(python::DocValue::Literal(value)) => Document {
                    root: value,
                    anchors: Vec::new(),
                    warnings: Vec::new(),
                },
            };
            docs.errors.retain(|e| e.section != Some(key));
            let kept = docs.keep(document, None);
            if key == "DOCUMENTATION" {
                docs.doc = kept;
            } else {
                docs.returns = kept;
            }
        }
        docs
    }

    /// The docs ansible-core shows for a plugin whose docs file is `docfile`: that file's, or, if its
    /// `DOCUMENTATION` is empty, the first file in `dir` named for the plugin, `base`, with each of
    /// `DOC_EXTENSIONS`, that has any (`get_plugin_docs` and `_find_adjacent`). A file whose `DOCUMENTATION`
    /// can't be read is taken, its error kept, as ansible-core stops at it. None if none has any.
    fn plugin_docs(&mut self, docfile: Option<&str>, dir: &str, base: &str) -> Option<Docs> {
        let docfile = docfile?;
        let mut last = self.file_docs(docfile);
        if last.has_documentation(true) {
            return Some(last);
        }
        for ext in DOC_EXTENSIONS {
            let candidate = format!("{dir}{base}{ext}");
            if candidate == docfile || !self.source.has(&candidate) {
                continue;
            }
            last = self.file_docs(&candidate);
            if last.has_documentation(false) {
                return Some(last);
            }
        }
        last.has_documentation(false).then_some(last)
    }

    /// The file a module, `<dir><base>`, has its docs in, as `find_plugin_docfile` finds it: the first of
    /// `<base>.*` in sorted order, which is how the module loader picks among them, or, as that's often a
    /// `.ps1` or another language with no docs in it, the first file beside it with docs' extension.
    fn module_docfile(&self, dir: &str, base: &str) -> Option<String> {
        let prefix = format!("{dir}{base}.");
        let resolved = self.source.files.iter().find(|f| {
            f.strip_prefix(&prefix)
                .is_some_and(|rest| !rest.contains('/') && !MODULE_IGNORE_EXTS.iter().any(|e| f.ends_with(e)))
        })?;
        if resolved.ends_with(".py") {
            return Some(resolved.clone());
        }
        DOC_EXTENSIONS
            .iter()
            .map(|ext| format!("{dir}{base}{ext}"))
            .find(|path| self.source.has(path))
    }
}

/// Docs of a YAML sidecar, such as `plugins/modules/<name>.yml`: its `DOCUMENTATION`, `EXAMPLES` and `RETURN`. One
/// that isn't YAML, or holds something other than a mapping, which ansible-core fails on, is an error of the whole
/// file. An empty one has none.
fn sidecar_docs(source: &Source, path: &str) -> Docs {
    let mut docs = Docs::new(path);
    let Some(data) = source.read(path) else {
        return docs;
    };
    let text = String::from_utf8_lossy(&data).into_owned();
    let document = match yaml_document(&text) {
        Ok(document) => document,
        Err(why) => {
            docs.errors.push(DocsError {
                section: None,
                message: why,
                text: Some(text),
            });
            return docs;
        }
    };
    let root = Raw {
        doc: &document,
        value: &document.root,
    };
    if !root.is_mapping() {
        if truthy(root.resolved()) {
            docs.errors.push(DocsError {
                section: None,
                message: "isn't a mapping".to_owned(),
                text: Some(text),
            });
        }
        return docs;
    }
    docs.examples = root.get("EXAMPLES").and_then(|e| e.text()).map(str::to_owned);
    docs.doc = root.get("DOCUMENTATION").map(|r| (0, r.value.clone()));
    docs.returns = root.get("RETURN").map(|r| (0, r.value.clone()));
    docs.documents.push(document);
    docs
}

/// YAML from a collection, or why it isn't YAML Ansible could read.
fn yaml_document(text: &str) -> Result<Document, String> {
    crate::yaml::loads(text).map_err(|e| e.to_string())
}

/// A role's argument spec, as ansible-core reads it (`RoleMixin` in `cli/doc.py`, and `_get_role_argspecs` in
/// `playbook/role/__init__.py`): the `argument_specs` of the first of these that exists, and nothing if it has
/// none, even where a later one does. JSON is read as YAML, as ansible-core reads it.
const ROLE_SPEC_FILES: [&str; 6] = [
    "meta/argument_specs.yml",
    "meta/argument_specs.yaml",
    "meta/argument_specs.json",
    "meta/main.yml",
    "meta/main.yaml",
    "meta/main.json",
];

fn role_spec(source: &Source, role: &str) -> Option<Docs> {
    let path = ROLE_SPEC_FILES
        .iter()
        .map(|meta| format!("roles/{role}/{meta}"))
        .find(|path| source.has(path))?;
    let document = yaml(&source.read(&path).unwrap_or_default())?;
    let root = Raw {
        doc: &document,
        value: &document.root,
    };
    if !root.get("argument_specs")?.is_mapping() {
        return None;
    }
    let mut docs = Docs::new(&path);
    docs.entry_points = docs.keep(document, Some("argument_specs"));
    Some(docs)
}

/// `plugins/modules/a/b.py` is `a.b`, as collections allow subdirectories.
fn plugin_name(rel: &str) -> String {
    rel.rsplit_once('.').map_or(rel, |(stem, _)| stem).replace('/', ".")
}

/// The first of `candidates` that exists, and its contents.
fn runtime_file(source: &Source, candidates: &[&str]) -> (String, Option<Vec<u8>>) {
    for path in candidates {
        if let Some(data) = source.read(path) {
            return ((*path).to_owned(), Some(data));
        }
    }
    (candidates[0].to_owned(), None)
}

/// A collection's namespace, name and version, and its dependencies.
type Names = (Option<String>, Option<String>, Option<String>, Vec<(String, String)>);

/// The names from `MANIFEST.json`, or `galaxy.yml` in a source checkout.
fn metadata(source: &Source) -> Result<Names, ExtractError> {
    let metadata = match source.read(MANIFEST) {
        Some(data) => Some(
            Manifest::parse(&data)
                .map_err(|e| ExtractError::Metadata(MANIFEST, e))?
                .collection_info,
        ),
        None => match source.read(GALAXY) {
            Some(data) => Some(Metadata::from_galaxy_yml(&data).map_err(|e| ExtractError::Metadata(GALAXY, e))?),
            None => None,
        },
    };
    let Some(metadata) = metadata else {
        return Ok((None, None, None, Vec::new()));
    };
    let doc = &metadata.document;
    let text = |key: &str| match metadata.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let dependencies = match metadata.get("dependencies") {
        Some(Value::Mapping(entries)) => entries
            .iter()
            .filter_map(|(k, v)| match (doc.resolve(k), doc.resolve(v)) {
                (Value::String(k), Value::String(v)) => Some((k.clone(), v.clone())),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok((text("namespace"), text("name"), text("version"), dependencies))
}

const MANIFEST: &str = "MANIFEST.json";
const GALAXY: &str = "galaxy.yml";

/// Why a collection wasn't extracted.
#[derive(Debug)]
pub enum ExtractError {
    /// The directory can't be read.
    Io(io::Error),
    /// The file named, `MANIFEST.json` or `galaxy.yml`, can't be read as one. `MANIFEST.json` is read as
    /// `Manifest::parse` reads it, so one ansible-galaxy wouldn't have built is refused. Their values aren't checked.
    Metadata(&'static str, ReadError),
}

impl From<io::Error> for ExtractError {
    fn from(err: io::Error) -> Self {
        ExtractError::Io(err)
    }
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Io(err) => err.fmt(f),
            ExtractError::Metadata(file, err) => write!(f, "{file}: {err}"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Extract the collection in the directory `root`. With `docs`, each plugin's docs and the collection's doc fragments
/// are read as well.
pub fn extract(root: &Path, docs: bool) -> Result<Collection, ExtractError> {
    let source = Source::new(root)?;
    let (namespace, collection, version, dependencies) = metadata(&source)?;
    let mut reader = Reader {
        source: &source,
        issues: Issues::default(),
        trees: HashMap::new(),
    };
    let mut found = Found {
        full: docs,
        ..Found::default()
    };
    let mut fragments: Vec<(String, Vec<(String, Value)>)> = Vec::new();
    // Each filter and test name, with the file providing it: the last one in sorted order, as the filter loader goes
    // through a directory's files in order and each one defining the name replaces the one before.
    let mut providers: Vec<((String, String), String)> = Vec::new();
    let mut provider_index: HashMap<(String, String), usize> = HashMap::new();

    let mut module_names: HashSet<String> = HashSet::new();
    for path in &source.files {
        let parts: Vec<&str> = path.split('/').collect();

        if parts[0] == "plugins" && parts.len() >= 3 {
            let pdir = parts[1];
            let sub = parts[2..].join("/");
            let Some((stem, ext)) = sub.rsplit_once('.') else {
                continue;
            };
            if stem.is_empty() || stem.ends_with("__init__") || !matches!(ext, "py" | "ps1") {
                continue;
            }
            // The plugin's directory, with its trailing /, and its name within it.
            let (dir, base) = match path.rsplit_once('/') {
                Some((dir, file)) => (format!("{dir}/"), file.rsplit_once('.').map_or(file, |(b, _)| b)),
                None => continue,
            };

            if pdir == "modules" {
                // A module is often both <name>.ps1 and a <name>.py doc stub; handle the pair once.
                let name = plugin_name(&sub);
                if !module_names.insert(name.clone()) {
                    continue;
                }
                let docfile = reader.module_docfile(&dir, base);
                let module_docs = reader.plugin_docs(docfile.as_deref(), &dir, base);
                found.add("module", name, module_docs, &mut reader.issues);
            } else if matches!(pdir, "filter" | "test") && ext == "py" {
                let names = reader
                    .tree(path)
                    .and_then(|t| python::filter_test_names(&t, pdir == "filter"));
                let Some(names) = names else {
                    reader.issues.add("filter_test_unresolved", path, "");
                    continue;
                };
                let prefix = sub.rsplit_once('/').map_or("", |(p, _)| p).replace('/', ".");
                for n in names {
                    let key = (
                        pdir.to_owned(),
                        if prefix.is_empty() { n } else { format!("{prefix}.{n}") },
                    );
                    match provider_index.get(&key) {
                        Some(&i) => providers[i].1 = path.clone(),
                        None => {
                            provider_index.insert(key.clone(), providers.len());
                            providers.push((key, path.clone()));
                        }
                    }
                }
            } else if pdir == "doc_fragments" && ext == "py" && docs {
                let (attributes, unresolved) = match reader.tree(path) {
                    Some(tree) => python::fragment_attributes(&tree),
                    None => (Vec::new(), true),
                };
                if unresolved {
                    reader.issues.add("fragment_unresolved", path, "");
                }
                let mut fragment = Vec::new();
                for (attr, text) in attributes {
                    let Some(document) = yaml(text.as_bytes()) else {
                        continue;
                    };
                    let raw = Raw {
                        doc: &document,
                        value: &document.root,
                    };
                    if let Some(value) = json_mapping(Some(&raw), path, &attr, &mut reader.issues) {
                        fragment.push((attr, value));
                    }
                }
                if !fragment.is_empty() {
                    let name = plugin_name(&sub);
                    match fragments.iter_mut().find(|(n, _)| *n == name) {
                        Some(existing) => existing.1 = fragment,
                        None => fragments.push((name, fragment)),
                    }
                }
            } else if let Some((_, kind)) = PLUGIN_DIRS.iter().find(|(dir, _)| *dir == pdir)
                && ext == "py"
            {
                // Found by its name with .py, as the controller's plugin loaders find them.
                let plugin_docs = reader.plugin_docs(Some(path), &dir, base);
                found.add(kind, plugin_name(&sub), plugin_docs, &mut reader.issues);
            }
        } else if parts[0] == "roles" && parts.len() >= 3 {
            let role = parts[1];
            if found.has("role", role) {
                continue;
            }
            found.add("role", role.to_owned(), role_spec(&source, role), &mut reader.issues);
        } else if parts[0] == "playbooks" && parts.len() == 2 && (path.ends_with(".yml") || path.ends_with(".yaml")) {
            // site.yml and site.yaml are the same playbook, and playbooks have no docs.
            let name = parts[1].rsplit_once('.').map_or(parts[1], |(stem, _)| stem);
            found.add("playbook", name.to_owned(), None, &mut reader.issues);
        }
    }

    // A filter's or test's docs file is <name>.py in its directory if there's one, as the loader finds the name there
    // first, or else the file providing it (find_plugin_docfile).
    for ((kind, name), provider) in providers {
        let (dir, _) = provider.rsplit_once('/').expect("a provider is in plugins/<type>/");
        let dir = format!("{dir}/");
        let base = name.rsplit_once('.').map_or(name.as_str(), |(_, b)| b).to_owned();
        let own = format!("{dir}{base}.py");
        let docfile = if source.has(&own) { own } else { provider };
        let filter_docs = reader.plugin_docs(Some(&docfile), &dir, &base);
        found.add(&kind, name, filter_docs, &mut reader.issues);
    }

    // Action plugins without a module of the same name are run as modules. Their docs are in the action plugin or, as
    // alai has them, as ansible-doc doesn't document such a module, a plugins/modules/<name>.yml sidecar.
    for path in &source.files {
        let Some(sub) = path.strip_prefix("plugins/action/").and_then(|p| p.strip_suffix(".py")) else {
            continue;
        };
        let name = plugin_name(&format!("{sub}.py"));
        if name.ends_with("__init__") || module_names.contains(&name) {
            continue;
        }
        let (subdir, base) = sub.rsplit_once('/').map_or(("", sub), |(d, b)| (d, b));
        let modules_dir = if subdir.is_empty() {
            "plugins/modules/".to_owned()
        } else {
            format!("plugins/modules/{subdir}/")
        };
        // An action plugin with no DOCUMENTATION it can read still has its sidecar, which ansible-core would never
        // look at.
        let mut action_docs = reader.plugin_docs(Some(path), &modules_dir, base);
        if action_docs.as_ref().is_none_or(|d| d.documentation().is_none()) {
            let sidecar = [".yml", ".yaml"]
                .iter()
                .map(|ext| format!("{modules_dir}{base}{ext}"))
                .find(|p| source.has(p));
            if let Some(found) = reader.plugin_docs(sidecar.as_deref(), &modules_dir, base) {
                action_docs = Some(found);
            }
        }
        found.add("module", name, action_docs, &mut reader.issues);
    }

    // Event-Driven Ansible: a name in more than one directory is only taken from the first, which is the file
    // ansible-rulebook loads. Its docs are read as a module's, as galaxy-importer reads them, with ansible-doc -M.
    for (directory, kind, ext) in EDA_DIRS {
        for path in &source.files {
            let Some(name) = path.strip_prefix(directory).and_then(|p| p.strip_suffix(ext)) else {
                continue;
            };
            if name.contains('/') || name.contains('.') || name == "__init__" || found.has(kind, name) {
                continue;
            }
            if ext == ".yml" {
                // Rulebooks, like playbooks, have no docs.
                found.add(kind, name.to_owned(), None, &mut reader.issues);
            } else {
                let docfile = reader.module_docfile(directory, name);
                let eda_docs = reader.plugin_docs(docfile.as_deref(), directory, name);
                found.add(kind, name.to_owned(), eda_docs, &mut reader.issues);
            }
        }
    }
    let mut issues = reader.issues;

    let mut routing = Vec::new();
    let mut fragment_routing: Vec<(String, Value)> = Vec::new();
    let (runtime_path, runtime_data) = runtime_file(&source, &["meta/runtime.yml", "meta/runtime.yaml"]);
    let runtime = runtime_data.and_then(|data| Runtime::parse(&data).ok());
    if let Some(runtime) = &runtime {
        let doc = &runtime.plugin_routing.document;
        for (key, entries) in &runtime.plugin_routing.types {
            let kind = match key.as_str() {
                "modules" => "module",
                "filter" => "filter",
                "test" => "test",
                "doc_fragments" if docs => "doc_fragments",
                other => match PLUGIN_DIRS.iter().find(|(dir, _)| *dir == other) {
                    Some((_, kind)) => kind,
                    None => continue,
                },
            };
            for (name, _, raw) in entries {
                let Some(entry) = json_entry(doc, raw, &runtime_path, kind, name, &mut issues) else {
                    continue;
                };
                if kind == "doc_fragments" {
                    fragment_routing.push((name.clone(), entry));
                } else {
                    routing.push(Routing {
                        kind: kind.to_owned(),
                        name: name.clone(),
                        entry,
                    });
                }
            }
        }
    }
    let (eda_path, eda_data) = runtime_file(&source, &["extensions/eda/eda_runtime.yml"]);
    if let Some(eda) = eda_data.and_then(|data| EdaRuntime::parse(&data).ok()) {
        let doc = &eda.plugin_routing.document;
        for (kind, entries) in &eda.plugin_routing.types {
            if !EDA_ROUTING_TYPES.contains(&kind.as_str()) {
                continue;
            }
            for (name, _, raw) in entries {
                if let Some(entry) = json_entry(doc, raw, &eda_path, kind, name, &mut issues) {
                    routing.push(Routing {
                        kind: kind.clone(),
                        name: name.clone(),
                        entry,
                    });
                }
            }
        }
    }

    let fqcn = match (&namespace, &collection) {
        (Some(namespace), Some(name)) if !namespace.is_empty() && !name.is_empty() => {
            Some(format!("{namespace}.{name}"))
        }
        _ => None,
    };
    let requires_ansible = runtime.and_then(|r| match r.requires_ansible {
        Some(Value::String(s)) => Some(s),
        _ => None,
    });
    let Found {
        mut plugins,
        docs: mut plugin_docs,
        ..
    } = found;
    plugins.sort_by(|a, b| a.0.cmp(&b.0));
    routing.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
    // A name given twice keeps its last entry, as a dict built in order does.
    fragment_routing.reverse();
    let mut seen = HashSet::new();
    fragment_routing.retain(|(name, _)| seen.insert(name.clone()));
    fragment_routing.sort_by(|a, b| a.0.cmp(&b.0));
    fragments.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Collection {
        namespace,
        name: collection,
        version,
        dependencies,
        requires_ansible,
        plugins: plugins
            .into_iter()
            .map(|((kind, name), short_description)| {
                let docs = plugin_docs
                    .remove(&(kind.clone(), name.clone()))
                    .map(|docs| PluginDocs {
                        collection: fqcn.clone(),
                        module: kind == "module",
                        ..docs
                    });
                Plugin {
                    kind,
                    name,
                    short_description,
                    docs,
                }
            })
            .collect(),
        routing,
        fragments: fragments
            .into_iter()
            .map(|(name, attributes)| DocFragment {
                collection: fqcn.clone(),
                name,
                attributes,
            })
            .collect(),
        fragment_routing,
        issues: issues.0,
    })
}

/// A routing entry as a JSON-safe mapping, or None if it's empty, or too large, which is an issue.
fn json_entry(doc: &Document, raw: &Value, path: &str, kind: &str, name: &str, issues: &mut Issues) -> Option<Value> {
    if matches!(doc.resolve(raw), Value::Mapping(entries) if entries.is_empty()) {
        return None;
    }
    let value = json_value(doc, raw);
    if value.is_none() {
        issues.add("value_too_large", path, &format!("{kind}:{name}"));
    }
    value
}

/// The plugins found so far, by type and name, each with its description and, with docs, its docs.
#[derive(Default)]
struct Found {
    full: bool,
    plugins: Vec<((String, String), Option<String>)>,
    index: HashMap<(String, String), usize>,
    docs: HashMap<(String, String), PluginDocs>,
}

impl Found {
    /// A plugin found again keeps its first docs, and a later description only fills a gap.
    fn add(&mut self, kind: &str, name: String, found: Option<Docs>, issues: &mut Issues) {
        let desc = found.as_ref().and_then(Docs::short_description);
        // A Python file that doesn't parse is already a python_unparseable issue.
        for (docs, error) in found.iter().flat_map(|d| d.errors.iter().map(move |e| (d, e))) {
            if error.section.is_some() || !docs.path.ends_with(".py") {
                let detail = match error.section {
                    Some(section) => format!("{section}: {}", error.message),
                    None => error.message.clone(),
                };
                issues.add("docs_unreadable", &docs.path, &detail);
            }
        }
        let docs = if self.full {
            found.and_then(|d| d.plugin_docs(issues))
        } else {
            None
        };
        let key = (kind.to_owned(), name);
        match self.index.get(&key) {
            Some(&i) => {
                if self.plugins[i].1.is_none() {
                    self.plugins[i].1 = desc;
                }
            }
            None => {
                self.index.insert(key.clone(), self.plugins.len());
                self.plugins.push((key.clone(), desc));
            }
        }
        if let Some(docs) = docs {
            self.docs.entry(key).or_insert(docs);
        }
    }

    fn has(&self, kind: &str, name: &str) -> bool {
        self.index.contains_key(&(kind.to_owned(), name.to_owned()))
    }
}
