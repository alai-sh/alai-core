// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! Applying doc fragments the way ansible-core does when it loads a plugin's docs. The merge and the collection
//! labels follow ansible-core's `lib/ansible/utils/plugin_docs.py` (`add_fragments`, `merge_fragment` and
//! `add_collection_to_versions_and_dates`).

use std::collections::{HashMap, HashSet};

use crate::yaml::Value;

/// A doc fragment, `plugins/doc_fragments/<name>.py`, as the collection wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct DocFragment {
    /// The collection defining it, `ns.coll`, or None if its name is unknown.
    pub collection: Option<String>,
    /// Its name within the collection, such as `auth`, or `sub.auth` in a subdirectory.
    pub name: String,
    /// `ModuleDocFragment`'s string attributes, parsed: `DOCUMENTATION`, `RETURN` and any other uppercase name,
    /// which `ns.coll.auth.other` refers to. Each is a mapping.
    pub attributes: Vec<(String, Value)>,
}

const SECTIONS: [&str; 2] = ["DOCUMENTATION", "RETURN"];

/// Labels a mapping's version or date field with the collection it belongs to.
type Label<'a> = dyn Fn(&mut Vec<(Value, Value)>, &str) + 'a;

/// A fragment's full name: a short one, such as `files`, is ansible.builtin's.
pub fn full_name(name: &str) -> String {
    if name.matches('.').count() >= 2 {
        name.to_owned()
    } else {
        format!("ansible.builtin.{name}")
    }
}

pub(crate) fn get<'a>(entries: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(k, _)| matches!(k, Value::String(s) if s == key))
        .map(|(_, v)| v)
}

fn get_mut<'a>(entries: &'a mut [(Value, Value)], key: &str) -> Option<&'a mut Value> {
    entries
        .iter_mut()
        .find(|(k, _)| matches!(k, Value::String(s) if s == key))
        .map(|(_, v)| v)
}

fn has(entries: &[(Value, Value)], key: &Value) -> bool {
    entries.iter().any(|(k, _)| k == key)
}

/// Set `key` as a Python dict does: in place if it's there, appended if it isn't.
fn set(entries: &mut Vec<(Value, Value)>, key: Value, value: Value) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => entries.push((key, value)),
    }
}

fn pop(entries: &mut Vec<(Value, Value)>, key: &str) -> Option<Value> {
    let index = entries
        .iter()
        .position(|(k, _)| matches!(k, Value::String(s) if s == key))?;
    Some(entries.remove(index).1)
}

/// The fragment `name` refers to, and the value it adds to `section`. `ns.coll.frag` is the fragment's
/// `DOCUMENTATION` (or `RETURN`). If no fragment has the whole name, the last part names an attribute instead:
/// `ns.coll.frag.facts` is the `FACTS` attribute of `ns.coll.frag`.
fn find<E>(
    name: &str,
    section: &str,
    lookup: &mut impl FnMut(&str) -> Result<Option<DocFragment>, E>,
) -> Result<Option<(DocFragment, Value)>, E> {
    let mut fragment = lookup(&full_name(name))?;
    let mut var = section.to_owned();
    if fragment.is_none()
        && let Some((base, suffix)) = name.rsplit_once('.')
    {
        fragment = lookup(&full_name(base))?;
        var = suffix.to_uppercase();
    }
    let Some(fragment) = fragment else {
        return Ok(None);
    };
    let value = match fragment.attributes.iter().find(|(k, _)| *k == var) {
        Some((_, value)) => value.clone(),
        // A fragment without RETURN, for instance, adds nothing to RETURN.
        None if SECTIONS.contains(&var.as_str()) => Value::Mapping(Vec::new()),
        None => return Ok(None),
    };
    Ok(Some((fragment, value)))
}

/// `target`, `DOCUMENTATION` or `RETURN` as `section` says, with the fragments `names` applied, and the names
/// `lookup` couldn't supply. `tag` is the plugin's collection and whether it's a module, to label versions with.
pub fn apply_section<E>(
    target: Option<&Value>,
    names: &[String],
    section: &str,
    lookup: &mut impl FnMut(&str) -> Result<Option<DocFragment>, E>,
    tag: Option<(&str, bool)>,
) -> Result<(Option<Value>, Vec<String>), E> {
    if names.is_empty() && (tag.is_none() || target.is_none()) {
        return Ok((target.cloned(), names.to_vec()));
    }
    let returns = section == "RETURN";
    let mut doc = match target {
        Some(Value::Mapping(entries)) => entries.clone(),
        _ => Vec::new(),
    };
    if let Some((collection, module)) = tag {
        label_versions(&mut doc, collection, module, returns);
    }
    let mut missing = Vec::new();
    for name in names {
        let Some((found, value)) = find(name, section, lookup)? else {
            missing.push(name.clone());
            continue;
        };
        let Value::Mapping(mut fragment) = value else {
            continue;
        };
        if let (Some((_, module)), Some(collection)) = (tag, &found.collection) {
            label_versions(&mut fragment, collection, module, returns);
        }
        if !returns {
            // A fragment's notes and seealso only ever extend the plugin's; an empty one (community.general's
            // _hpe3par has `notes:`) adds nothing.
            for key in ["notes", "seealso"] {
                if let Some(Value::Sequence(entries)) = pop(&mut fragment, key)
                    && !entries.is_empty()
                {
                    let mut combined = match get(&doc, key) {
                        Some(Value::Sequence(current)) => current.clone(),
                        _ => Vec::new(),
                    };
                    combined.extend(entries);
                    set(&mut doc, Value::String(key.to_owned()), Value::Sequence(combined));
                }
            }
            for key in ["options", "attributes"] {
                if let (Some(Value::Mapping(current)), Some(Value::Mapping(value))) =
                    (get_mut(&mut doc, key), get(&fragment, key))
                {
                    let value = value.clone();
                    merge(current, value);
                    pop(&mut fragment, key);
                }
            }
        }
        merge(&mut doc, fragment);
    }
    Ok((Some(Value::Mapping(doc)), missing))
}

/// ansible-core's `merge_fragment`: `target`'s values win, mappings merge one level, and lists combine.
fn merge(target: &mut Vec<(Value, Value)>, source: Vec<(Value, Value)>) {
    for (key, value) in source {
        if !has(target, &key) {
            target.push((key, value));
            continue;
        }
        let current = target
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
            .expect("checked above");
        match (&*current, value) {
            (Value::Mapping(current_entries), Value::Mapping(mut entries)) => {
                // {**value, **current}: the fragment's keys first, the plugin's values winning.
                for (k, v) in current_entries {
                    set(&mut entries, k.clone(), v.clone());
                }
                *current = Value::Mapping(entries);
            }
            (Value::Sequence(current_items), Value::Sequence(items)) => {
                *current = Value::Sequence(combine(current_items, items));
            }
            _ => {}
        }
    }
}

/// `sorted(set(source + target))`, as ansible-core combines lists, or where Python can't (mappings or lists in it,
/// or values that don't compare), which fails in ansible-core, every distinct item, the plugin's first.
fn combine(target: &[Value], source: Vec<Value>) -> Vec<Value> {
    let items: Vec<&Value> = source.iter().chain(target).collect();
    let mut unique: Vec<&Value> = Vec::new();
    let hashable = !items
        .iter()
        .any(|v| matches!(v, Value::Mapping(_) | Value::Sequence(_)));
    if hashable {
        for item in &items {
            if !unique.iter().any(|u| same(u, item)) {
                unique.push(item);
            }
        }
        if unique.len() <= 1 {
            return unique.into_iter().cloned().collect();
        }
        if unique.iter().all(|v| matches!(v, Value::String(_))) {
            unique.sort_by(|a, b| text(a).cmp(text(b)));
            return unique.into_iter().cloned().collect();
        }
        if unique.iter().all(|v| number(v).is_some()) {
            unique.sort_by(|a, b| number(a).partial_cmp(&number(b)).unwrap_or(std::cmp::Ordering::Equal));
            return unique.into_iter().cloned().collect();
        }
    }
    let mut out = target.to_vec();
    for item in source {
        if !target.iter().any(|t| same(t, &item)) {
            out.push(item);
        }
    }
    out
}

fn text(value: &Value) -> &str {
    match value {
        Value::String(s) => s,
        _ => "",
    }
}

/// A value as Python compares numbers, where `True == 1 == 1.0`.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        Value::BigInt(digits) => digits.parse().ok(),
        _ => None,
    }
}

/// Equal as Python compares them, numbers by value.
fn same(a: &Value, b: &Value) -> bool {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// ansible-core's `add_collection_to_versions_and_dates`: name the collection each version and date belongs to,
/// where the docs don't already.
fn label_versions(doc: &mut Vec<(Value, Value)>, collection: &str, module: bool, returns: bool) {
    let label = |entry: &mut Vec<(Value, Value)>, field: &str| {
        let key = Value::String(field.to_owned());
        if !has(entry, &key) {
            entry.push((key, Value::String(collection.to_owned())));
        }
    };
    let deprecation = |value: Option<&mut Value>, top_level: bool| {
        let Some(Value::Mapping(entry)) = value else {
            return;
        };
        let key = |k: &str| has(entry, &Value::String(k.to_owned()));
        // Module deprecations use removed_in, plugin option deprecations version.
        if (key("removed_in") && (module || top_level))
            || key("removed_at_date")
            || (key("version") && !(module || top_level))
        {
            label(
                entry,
                if top_level {
                    "removed_from_collection"
                } else {
                    "collection_name"
                },
            );
        }
    };
    fn mappings(value: Option<&mut Value>) -> Vec<&mut Vec<(Value, Value)>> {
        match value {
            Some(Value::Mapping(entries)) => entries
                .iter_mut()
                .filter_map(|(_, v)| match v {
                    Value::Mapping(m) => Some(m),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    fn options(
        values: Option<&mut Value>,
        module: bool,
        label: &Label<'_>,
        deprecation: &dyn Fn(Option<&mut Value>, bool),
    ) {
        for option in mappings(values) {
            if get(option, "version_added").is_some() {
                label(option, "version_added_collection");
            }
            if !module {
                for key in ["env", "ini", "vars"] {
                    if let Some(Value::Sequence(items)) = get_mut(option, key) {
                        for item in items {
                            let Value::Mapping(specifier) = item else {
                                continue;
                            };
                            if get(specifier, "version_added").is_some() {
                                label(specifier, "version_added_collection");
                            }
                            deprecation(get_mut(specifier, "deprecated"), false);
                        }
                    }
                }
                deprecation(get_mut(option, "deprecated"), false);
            }
            options(get_mut(option, "suboptions"), module, label, deprecation);
        }
    }
    fn return_values(values: Option<&mut Value>, label: &Label<'_>) {
        for value in mappings(values) {
            if get(value, "version_added").is_some() {
                label(value, "version_added_collection");
            }
            return_values(get_mut(value, "contains"), label);
        }
    }
    if returns {
        let mut whole = Value::Mapping(std::mem::take(doc));
        return_values(Some(&mut whole), &label);
        if let Value::Mapping(entries) = whole {
            *doc = entries;
        }
        return;
    }
    if get(doc, "version_added").is_some() {
        label(doc, "version_added_collection");
    }
    deprecation(get_mut(doc, "deprecated"), true);
    options(get_mut(doc, "options"), module, &label, &deprecation);
    for attribute in mappings(get_mut(doc, "attributes")) {
        if get(attribute, "version_added").is_some() {
            label(attribute, "version_added_collection");
        }
    }
}

/// Looks fragments up across collections, following their `doc_fragments` routing as ansible-core does: routing is
/// checked first, a redirect is followed into any of the collections, and a tombstone means the fragment was removed.
#[derive(Clone, Debug, Default)]
pub struct FragmentIndex {
    fragments: HashMap<String, DocFragment>,
    routing: HashMap<String, Value>,
}

impl FragmentIndex {
    /// Add a collection's fragments and their routing, under `ns.coll.`, replacing any of the same name.
    pub fn add(&mut self, collection: &str, fragments: &[DocFragment], routing: &[(String, Value)]) {
        for fragment in fragments {
            self.fragments
                .insert(format!("{collection}.{}", fragment.name), fragment.clone());
        }
        for (name, entry) in routing {
            self.routing.insert(format!("{collection}.{name}"), entry.clone());
        }
    }

    /// The fragment of the full name `name`, or None if it isn't available, was removed, or redirects in a loop.
    pub fn get(&self, name: &str) -> Option<&DocFragment> {
        let mut name = name.to_owned();
        let mut seen = HashSet::new();
        while seen.insert(name.clone()) {
            let entry = match self.routing.get(&name) {
                Some(Value::Mapping(entry)) => entry.as_slice(),
                _ => &[],
            };
            if get(entry, "tombstone").is_some() {
                return None;
            }
            match get(entry, "redirect") {
                Some(Value::String(redirect)) => name = full_name(redirect),
                _ => return self.fragments.get(&name),
            }
        }
        None
    }
}
