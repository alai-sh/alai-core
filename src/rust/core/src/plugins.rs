// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! The plugin index, `plugins.jsonl`, as galaxy-sync's `docs/index-spec/plugins.md` specifies it.
//!
//! A search finds the lines it wants in the raw bytes and reads only those. A line that can't be read, or that needs
//! a newer reader, is skipped, as a plugin index is a search aid.

use std::collections::HashSet;

use memchr::memmem;
use serde_json::{Map, Value};

use crate::ParseError;
use crate::json::{is_blank, object, string};
use crate::names::check_collection_name;

/// The plugin index line `v` written, and the highest read. A `v` of 0 or less is still read.
pub const PLUGIN_LINE_VERSION: i64 = 1;

/// One line of the plugin index, a plugin, role or playbook a collection has or had.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    kind: String,
    name: String,
    collection: String,
    since: Option<String>,
    until: Option<String>,
    short_description: Option<String>,
    routing: Option<Map<String, Value>>,
}

impl PluginEntry {
    pub fn new(
        kind: &str,
        name: &str,
        collection: &str,
        since: Option<&str>,
        until: Option<&str>,
        short_description: Option<&str>,
        routing: Option<Map<String, Value>>,
    ) -> Result<Self, ParseError> {
        if kind.is_empty() || name.is_empty() {
            return Err(ParseError::new("a plugin's 'type' and 'name' must not be empty"));
        }
        check_collection_name(collection)?;
        Ok(PluginEntry {
            kind: kind.to_owned(),
            name: name.to_owned(),
            collection: collection.to_owned(),
            since: since.map(str::to_owned),
            until: until.map(str::to_owned),
            short_description: short_description.map(str::to_owned),
            routing,
        })
    }

    /// The line as a plugin, or None if it can't be read. It must be a JSON object with a `v` no higher than this
    /// reads, and `type`, `name` and `collection` strings, the collection with one dot.
    pub fn parse(line: &[u8]) -> Option<Self> {
        let obj = object(line).ok()?;
        if !obj
            .get("v")
            .and_then(Value::as_i64)
            .is_some_and(|v| v <= PLUGIN_LINE_VERSION)
        {
            return None;
        }
        let text = |key: &str| obj.get(key).and_then(Value::as_str).map(str::to_owned);
        let (kind, name, collection) = (text("type")?, text("name")?, text("collection")?);
        if collection.matches('.').count() != 1 {
            return None;
        }
        Some(PluginEntry {
            since: text("since"),
            until: text("until"),
            short_description: text("short_description"),
            routing: match obj.get("routing") {
                Some(Value::Object(routing)) => Some(routing.clone()),
                _ => None,
            },
            kind,
            name,
            collection,
        })
    }

    /// The type, such as `module` or `filter`, which may be one this code doesn't know.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn collection(&self) -> &str {
        &self.collection
    }

    /// The lowest version of the collection that has it.
    pub fn since(&self) -> Option<&str> {
        self.since.as_deref()
    }

    /// The version it was removed in, or None if the latest still has it.
    pub fn until(&self) -> Option<&str> {
        self.until.as_deref()
    }

    pub fn short_description(&self) -> Option<&str> {
        self.short_description.as_deref()
    }

    /// Its entry in the collection's `plugin_routing`, as the index copied it.
    pub fn routing(&self) -> Option<&Map<String, Value>> {
        self.routing.as_ref()
    }

    /// The line as the index stores it. Keys within `routing` are sorted.
    pub fn to_json(&self) -> Result<String, ParseError> {
        check_collection_name(&self.collection)?;
        let mut out = format!(
            "{{\"v\":{PLUGIN_LINE_VERSION},\"type\":{},\"name\":{},\"collection\":{}",
            string(&self.kind),
            string(&self.name),
            string(&self.collection),
        );
        for (key, value) in [
            ("since", &self.since),
            ("until", &self.until),
            ("short_description", &self.short_description),
        ] {
            if let Some(value) = value {
                out.push_str(&format!(",\"{key}\":{}", string(value)));
            }
        }
        if let Some(routing) = &self.routing {
            let routing = serde_json::to_string(routing).expect("a JSON object is always valid JSON");
            out.push_str(&format!(",\"routing\":{routing}"));
        }
        out.push('}');
        Ok(out)
    }
}

/// Every readable line that contains all of `tokens`, or every readable line if there are none, with the line it was
/// read from. The first token is searched for, so it should be the rarest.
pub fn search<'a>(data: &'a [u8], tokens: &[Vec<u8>]) -> Vec<(PluginEntry, &'a [u8])> {
    let read = |line: &'a [u8]| PluginEntry::parse(line).map(|entry| (entry, line));
    let Some((first, rest)) = tokens.split_first() else {
        return data
            .split(|&b| b == b'\n')
            .filter(|line| !is_blank(line))
            .filter_map(read)
            .collect();
    };
    let rest: Vec<_> = rest.iter().map(memmem::Finder::new).collect();
    let mut found = Vec::new();
    let mut at = 0;
    for hit in memmem::find_iter(data, first) {
        if hit < at {
            continue; // in a line already looked at
        }
        let (start, end) = line_at(data, hit);
        at = end + 1;
        let line = &data[start..end];
        if rest.iter().all(|token| token.find(line).is_some()) {
            found.extend(read(line));
        }
    }
    found
}

/// Every readable line with a `"name": "..."` whose value is one of `names`, each once, with the line it was read from.
pub fn named<'a>(data: &'a [u8], names: &HashSet<String>) -> Vec<(PluginEntry, &'a [u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    for (offset, value) in name_values(data) {
        if offset < at || !std::str::from_utf8(value).is_ok_and(|v| names.contains(v)) {
            continue;
        }
        let (start, end) = line_at(data, offset);
        at = end + 1;
        let line = &data[start..end];
        found.extend(PluginEntry::parse(line).map(|entry| (entry, line)));
    }
    found
}

/// Every `"name": "..."` value in `data` without an escape in it, read without parsing the lines.
pub fn names(data: &[u8]) -> HashSet<String> {
    name_values(data)
        .filter_map(|(_, value)| std::str::from_utf8(value).ok().map(str::to_owned))
        .collect()
}

/// The start and end of the line holding the byte at `offset`, its newline left out.
fn line_at(data: &[u8], offset: usize) -> (usize, usize) {
    let start = memchr::memrchr(b'\n', &data[..offset]).map_or(0, |i| i + 1);
    let end = memchr::memchr(b'\n', &data[offset..]).map_or(data.len(), |i| offset + i);
    (start, end)
}

/// Where each value of a `"name"` key in `data` is, and the value up to its closing quote. A value with an escape in
/// it isn't one, as no plugin name needs one.
fn name_values(data: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    const KEY: &[u8] = b"\"name\"";
    let mut keys = memmem::find_iter(data, KEY);
    std::iter::from_fn(move || {
        for found in keys.by_ref() {
            let mut i = found + KEY.len();
            while data.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if data.get(i) != Some(&b':') {
                continue;
            }
            i += 1;
            while data.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if data.get(i) != Some(&b'"') {
                continue;
            }
            let start = i + 1;
            let Some(len) = memchr::memchr2(b'"', b'\\', &data[start..]) else {
                continue;
            };
            if data[start + len] == b'\\' {
                continue;
            }
            // A value holds no '"', so the next key can't start inside this one.
            return Some((found, &data[start..start + len]));
        }
        None
    })
}
