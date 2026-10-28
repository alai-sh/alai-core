// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! TOML through `toml_edit`, as cargo and uv read and write it. Reading follows TOML 1.1, writing lays a table out the
//! same way every time, and a `Document` is edited keeping the rest of its text, as uv edits `pyproject.toml`. Every
//! edit is checked. The text it gives, read back, must be the document as it was with only that change made.

use std::collections::BTreeMap;
use std::fmt;

pub use toml_edit;
use toml_edit::{Array, ArrayOfTables, Datetime, DocumentMut, InlineTable, Item, RawString, Table, Value as TomlValue};

use crate::ParseError;

/// A value to write, its tables' keys in the order given.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
}

/// An edit that can't be made, or that wouldn't give exactly the intended document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError(String);

impl EditError {
    fn new(message: impl Into<String>) -> Self {
        EditError(message.into())
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

/// TOML text as a document. The error says where, as `tomllib` does, such as `(at line 3, column 7)`.
pub fn parse(text: &str) -> Result<DocumentMut, ParseError> {
    text.parse::<DocumentMut>()
        .map_err(|e| ParseError::new(message(text, &e)))
}

/// A file's bytes as text. Where it isn't UTF-8 is said as a syntax error's place is.
pub fn decode(bytes: Vec<u8>) -> Result<String, ParseError> {
    String::from_utf8(bytes).map_err(|e| {
        let valid = String::from_utf8_lossy(&e.as_bytes()[..e.utf8_error().valid_up_to()]);
        ParseError::new(format!("not UTF-8 {}", position(&valid, valid.len())))
    })
}

/// `table` as TOML, laid out the same way every time.
///
/// - A table in `table` is written as a `[key]` header line and then its own keys.
/// - A non-empty array in `table` whose items are all tables gets a `[[key]]` header line for each item where each
///   header follows a blank line.
/// - Every other value in `table` is a `key = value` line, written before the first header.
/// - Inside a table, a table of only strings, numbers and booleans is an inline table, `{ k = "v" }`. Any other table
///   is written as dotted keys, `a.b.c = ...`.
/// - An array is on one line if that's at most `width` characters, else one item per line with a trailing comma.
pub fn dumps(table: &[(String, Value)], width: usize) -> String {
    let mut doc = DocumentMut::new();
    let root = doc.as_table_mut();
    for (key, value) in table {
        match value {
            Value::Table(entries) => {
                let _ = root.insert(key, Item::Table(table_from(entries, width)));
            }
            Value::Array(items) if !items.is_empty() && items.iter().all(|v| matches!(v, Value::Table(_))) => {
                let mut tables = ArrayOfTables::new();
                for item in items {
                    if let Value::Table(entries) = item {
                        tables.push(table_from(entries, width));
                    }
                }
                let _ = root.insert(key, Item::ArrayOfTables(tables));
            }
            _ => {
                let _ = root.insert(key, Item::Value(value_from(value, width)));
            }
        }
    }
    doc.to_string()
}

fn table_from(entries: &[(String, Value)], width: usize) -> Table {
    let mut table = Table::new();
    for (key, value) in entries {
        let item = match value {
            Value::Table(inner) if !inner.iter().all(|(_, v)| is_scalar(v)) => {
                let mut dotted = table_from(inner, width);
                dotted.set_dotted(true);
                Item::Table(dotted)
            }
            _ => Item::Value(value_from(value, width)),
        };
        let _ = table.insert(key, item);
    }
    table
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Integer(_) | Value::Float(_) | Value::Boolean(_)
    )
}

fn value_from(value: &Value, width: usize) -> TomlValue {
    match value {
        Value::String(text) => TomlValue::from(text.as_str()),
        Value::Integer(n) => TomlValue::from(*n),
        Value::Float(n) => TomlValue::from(*n),
        Value::Boolean(flag) => TomlValue::from(*flag),
        Value::Table(entries) => {
            let mut table = InlineTable::new();
            for (key, v) in entries {
                let _ = table.insert(key, value_from(v, width));
            }
            TomlValue::InlineTable(table)
        }
        Value::Array(items) => {
            let mut array = Array::new();
            for v in items {
                array.push(value_from(v, width));
            }
            if TomlValue::Array(array.clone()).to_string().chars().count() > width {
                for item in array.iter_mut() {
                    item.decor_mut().set_prefix("\n    ");
                    item.decor_mut().set_suffix("");
                }
                array.set_trailing("\n");
                array.set_trailing_comma(true);
            }
            TomlValue::Array(array)
        }
    }
}

/// A TOML document being edited, such as `alai.toml` for `alai add` and `alai remove`.
#[derive(Clone, Debug)]
pub struct Document {
    doc: DocumentMut,
    /// What the document reads as. Every edit changes this too, and the text it gives must read as exactly this.
    expected: Plain,
    /// The text, as last checked.
    text: String,
    /// Whether every line ended in `\r\n`, which `toml_edit` writes as `\n`, so they're put back.
    crlf: bool,
}

impl Document {
    pub fn new(text: &str) -> Result<Self, ParseError> {
        let doc = parse(text)?;
        let newlines = text.matches('\n').count();
        Ok(Document {
            expected: Plain::of_table(doc.as_table()),
            doc,
            text: text.to_owned(),
            crlf: newlines > 0 && text.matches("\r\n").count() == newlines,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn document(&self) -> &DocumentMut {
        &self.doc
    }

    /// The array of strings at `path`, empty if there's no such key.
    pub fn values(&self, path: &[String]) -> Result<Vec<String>, EditError> {
        let Some(item) = self.item(path) else {
            return Ok(Vec::new());
        };
        let strings = item.as_array().and_then(|array| {
            array
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        });
        match strings {
            Some(strings) => Ok(strings),
            None => Err(EditError::new(format!("{} isn't an array of strings", dotted(path)))),
        }
    }

    /// Insert `value` at `index` in the array at `path`, as `uv add` does. Comments stay with the item they follow,
    /// and the array is then one item per line. Without the key, it's created as an array of just `value`, with the
    /// tables it's in. The one holding it gets a header, as `[project.groups.test]`, and those above it don't, unless
    /// they're dotted keys or inline.
    pub fn insert(&mut self, path: &[String], index: usize, value: &str) -> Result<(), EditError> {
        self.edit(|doc| {
            if doc.item(path).is_none() {
                return doc.create(path, value);
            }
            insert_formatted(doc.array(path)?, index, value);
            doc.expected_array(path)?.insert(index, Plain::String(value.to_owned()));
            doc.check(path)
        })
    }

    /// Replace the item at `index` in the array at `path` with `value`, keeping its comments.
    pub fn replace(&mut self, path: &[String], index: usize, value: &str) -> Result<(), EditError> {
        self.edit(|doc| {
            let array = doc.array(path)?;
            let _ = array.replace(index, value);
            reformat_multiline(array);
            doc.expected_array(path)?[index] = Plain::String(value.to_owned());
            doc.check(path)
        })
    }

    /// Remove the item at `index` from the array at `path`, as `uv remove` does. A comment on the line before stays
    /// there, and the array is then one item per line, or `[]`.
    pub fn remove(&mut self, path: &[String], index: usize) -> Result<(), EditError> {
        self.edit(|doc| {
            remove_at(doc.array(path)?, index);
            let _ = doc.expected_array(path)?.remove(index);
            doc.check(path)
        })
    }

    /// Set the key at `path` to an inline table of `entries`, in their order. It's set in place, keeping any comment
    /// after it, or added at the end of its table, created as `insert` creates one.
    pub fn set_table(&mut self, path: &[String], entries: &[(String, Value)]) -> Result<(), EditError> {
        self.edit(|doc| {
            let mut table = InlineTable::new();
            let mut plain = BTreeMap::new();
            for (key, v) in entries {
                let v = value_from(v, usize::MAX);
                let _ = plain.insert(key.clone(), Plain::of_value(&v));
                let _ = table.insert(key, v);
            }
            doc.set(path, TomlValue::InlineTable(table))?;
            doc.expected.set(path, Plain::Table(plain));
            doc.check(path)
        })
    }

    /// Remove the key at `path`, with any comment on its line or above it, as `uv remove` removes a source. Its table
    /// stays, even if that leaves it empty.
    pub fn remove_key(&mut self, path: &[String]) -> Result<(), EditError> {
        self.edit(|doc| {
            let (key, parents) = split_path(path)?;
            let mut item = doc.doc.as_item_mut();
            for name in parents {
                item = item
                    .as_table_like_mut()
                    .and_then(|table| table.get_mut(name))
                    .ok_or_else(|| EditError::new(format!("there's no {}", dotted(path))))?;
            }
            let removed = item.as_table_like_mut().and_then(|table| table.remove(key));
            if removed.is_none() {
                return Err(EditError::new(format!("there's no {}", dotted(path))));
            }
            doc.expected.remove(path);
            doc.check(path)
        })
    }

    /// Make an edit, or undo all of it if any part fails, so a refused edit never shows up in a later one.
    fn edit(&mut self, f: impl FnOnce(&mut Self) -> Result<(), EditError>) -> Result<(), EditError> {
        let result = f(self);
        if result.is_err() {
            *self = Self::new(&self.text).expect("the text was read or checked");
        }
        result
    }

    fn item(&self, path: &[String]) -> Option<&Item> {
        let mut item = self.doc.as_item();
        for key in path {
            item = item.get(key.as_str())?;
        }
        Some(item)
    }

    /// Set the key at `path` to an array of just `value`, one item per line.
    fn create(&mut self, path: &[String], value: &str) -> Result<(), EditError> {
        let mut array = Array::new();
        array.push(value);
        reformat_multiline(&mut array);
        self.set(path, TomlValue::Array(array))?;
        self.expected
            .set(path, Plain::Array(vec![Plain::String(value.to_owned())]));
        self.check(path)
    }

    /// The array at `path`, which must be there.
    fn array(&mut self, path: &[String]) -> Result<&mut Array, EditError> {
        let mut item = self.doc.as_item_mut();
        for key in path {
            match item.as_table_like_mut().and_then(|table| table.get_mut(key)) {
                Some(found) => item = found,
                None => return Err(EditError::new(format!("there's no {}", dotted(path)))),
            }
        }
        item.as_array_mut()
            .ok_or_else(|| EditError::new(format!("{} isn't an array", dotted(path))))
    }

    fn expected_array(&mut self, path: &[String]) -> Result<&mut Vec<Plain>, EditError> {
        match self.expected.get_mut(path) {
            Some(Plain::Array(items)) => Ok(items),
            _ => Err(EditError::new(format!("{} isn't an array", dotted(path)))),
        }
    }

    /// Set the key at `path` to `value`, keeping the decoration of the value it replaces, creating its tables.
    fn set(&mut self, path: &[String], mut value: TomlValue) -> Result<(), EditError> {
        let (key, parents) = split_path(path)?;
        let mut item = self.doc.as_item_mut();
        for (depth, name) in parents.iter().enumerate() {
            if item.get(name.as_str()).is_none() {
                let holder = depth + 1 == parents.len();
                match item {
                    Item::Table(table) => {
                        let mut child = Table::new();
                        child.set_dotted(table.is_dotted());
                        child.set_implicit(!holder);
                        let _ = table.insert(name, Item::Table(child));
                    }
                    Item::Value(TomlValue::InlineTable(table)) => {
                        let _ = table.insert(name, TomlValue::InlineTable(InlineTable::new()));
                    }
                    _ => return Err(not_a_table(path, depth)),
                }
            }
            item = item
                .as_table_like_mut()
                .and_then(|table| table.get_mut(name))
                .filter(|child| child.is_table_like())
                .ok_or_else(|| not_a_table(path, depth + 1))?;
        }
        let table = item
            .as_table_like_mut()
            .ok_or_else(|| not_a_table(path, parents.len()))?;
        match table.get_mut(key).and_then(Item::as_value_mut) {
            Some(old) => {
                *value.decor_mut() = old.decor().clone();
                *old = value;
            }
            None => {
                if let Item::Value(TomlValue::InlineTable(inline)) = item {
                    // The space before the closing brace is the last value's. It moves to the new last one, so the
                    // comma isn't written after it.
                    if let Some((_, last)) = inline.iter_mut().last() {
                        let suffix = last.decor().suffix().cloned();
                        last.decor_mut().set_suffix("");
                        if let Some(suffix) = suffix {
                            value.decor_mut().set_suffix(suffix);
                        }
                    }
                    let _ = inline.insert(key, value);
                } else if let Some(table) = item.as_table_like_mut() {
                    let _ = table.insert(key, Item::Value(value));
                }
            }
        }
        Ok(())
    }

    /// Render the edited document and check it reads as `expected`. It's only kept if it does.
    fn check(&mut self, path: &[String]) -> Result<(), EditError> {
        let mut text = self.doc.to_string();
        if self.crlf {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        match text.parse::<DocumentMut>() {
            Err(e) => Err(EditError::new(format!(
                "editing {} would leave the file unreadable: {}",
                dotted(path),
                message(&text, &e)
            ))),
            Ok(got) if Plain::of_table(got.as_table()) != self.expected => Err(EditError::new(format!(
                "editing {} would change more than it should",
                dotted(path)
            ))),
            Ok(_) => {
                self.text = text;
                Ok(())
            }
        }
    }
}

/// A TOML value as what it reads as, without its layout, to compare documents by.
#[derive(Clone, Debug, PartialEq)]
enum Plain {
    String(String),
    Integer(i64),
    /// The bits, so NaN equals itself.
    Float(u64),
    Boolean(bool),
    Datetime(Datetime),
    Array(Vec<Plain>),
    Table(BTreeMap<String, Plain>),
}

impl Plain {
    fn of_table(table: &dyn toml_edit::TableLike) -> Plain {
        Plain::Table(table.iter().map(|(k, v)| (k.to_owned(), Plain::of_item(v))).collect())
    }

    fn of_item(item: &Item) -> Plain {
        match item {
            Item::None => Plain::Table(BTreeMap::new()),
            Item::Value(value) => Plain::of_value(value),
            Item::Table(table) => Plain::of_table(table),
            Item::ArrayOfTables(tables) => Plain::Array(tables.iter().map(|t| Plain::of_table(t)).collect()),
        }
    }

    fn of_value(value: &TomlValue) -> Plain {
        match value {
            TomlValue::String(v) => Plain::String(v.value().clone()),
            TomlValue::Integer(v) => Plain::Integer(*v.value()),
            TomlValue::Float(v) => Plain::Float(v.value().to_bits()),
            TomlValue::Boolean(v) => Plain::Boolean(*v.value()),
            TomlValue::Datetime(v) => Plain::Datetime(*v.value()),
            TomlValue::Array(items) => Plain::Array(items.iter().map(Plain::of_value).collect()),
            TomlValue::InlineTable(table) => Plain::of_table(table),
        }
    }

    fn get_mut(&mut self, path: &[String]) -> Option<&mut Plain> {
        path.iter().try_fold(self, |node, key| match node {
            Plain::Table(table) => table.get_mut(key),
            _ => None,
        })
    }

    /// Set the key at `path`, creating the tables it's in.
    fn set(&mut self, path: &[String], value: Plain) {
        let Some((key, parents)) = path.split_last() else {
            return;
        };
        let mut node = self;
        for name in parents {
            let Plain::Table(table) = node else { return };
            node = table
                .entry(name.clone())
                .or_insert_with(|| Plain::Table(BTreeMap::new()));
        }
        if let Plain::Table(table) = node {
            let _ = table.insert(key.clone(), value);
        }
    }

    fn remove(&mut self, path: &[String]) {
        if let Some((key, parents)) = path.split_last()
            && let Some(Plain::Table(table)) = self.get_mut(parents)
        {
            let _ = table.remove(key);
        }
    }
}

/// `toml_edit`'s error as one line, with where it is as `tomllib` says it, such as `... (at line 3, column 7)`. The
/// column is in characters, from 1.
fn message(text: &str, err: &toml_edit::TomlError) -> String {
    let what = err.message().trim_end();
    let Some(span) = err.span() else {
        return what.to_owned();
    };
    format!("{what} {}", position(text, span.start))
}

/// Where byte `offset` of `text` is, as `tomllib` says it, such as `(at line 3, column 7)`.
fn position(text: &str, offset: usize) -> String {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or(before).chars().count() + 1;
    format!("(at line {line}, column {column})")
}

fn split_path(path: &[String]) -> Result<(&str, &[String]), EditError> {
    path.split_last()
        .map(|(key, parents)| (key.as_str(), parents))
        .ok_or_else(|| EditError::new("an empty key path"))
}

fn not_a_table(path: &[String], depth: usize) -> EditError {
    EditError::new(format!("{} isn't a table", dotted(&path[..depth.min(path.len())])))
}

/// A key path as it's written in TOML: `project.groups."my.group".collections`.
fn dotted(path: &[String]) -> String {
    path.iter()
        .map(|key| {
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                key.clone()
            } else {
                format!("{key:?}")
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

// What follows is uv's layout for a requirement array, from `uv-project-edit` (uv 0.13.5, `add_dependency`,
// `remove_dependency_at` and `reformat_array_multiline`), so `alai add` and `alai remove` leave an array as `uv add` and
// `uv remove` leave one: one item per line with a trailing comma, comments kept with the item they follow.

/// Insert `value` at `index`, moving comments so they stay with the lines they were on, as `uv add` does.
fn insert_formatted(array: &mut Array, index: usize, value: &str) {
    let mut value = TomlValue::from(value);
    let len = array.len();
    if index == len {
        // At the end: the comments after the last item, such as one at the end of its line, stay before the new one.
        value.decor_mut().set_prefix(array.trailing().clone());
        array.set_trailing("");
    } else if index > 0
        && let Some(next) = array.get_mut(index)
    {
        // Between two: the comment at the end of the line before stays there, before the new item.
        let prefix = next.decor().prefix().cloned().unwrap_or_default();
        next.decor_mut().set_prefix("");
        value.decor_mut().set_prefix(prefix);
    }
    array.insert_formatted(index, value);

    if array.len() > 1 && index == 0 {
        // At the front: a comment on the bracket's line stays there, and one on a line of its own moves down with
        // the item it was above.
        let prefix = array
            .get(1)
            .and_then(|v| v.decor().prefix())
            .cloned()
            .unwrap_or_default();
        match prefix
            .as_str()
            .and_then(|p| p.split_once(['\r', '\n']).map(|(first, rest)| (p, first, rest)))
        {
            Some((whole, first, rest)) => {
                let newline = match &whole[first.len()..] {
                    n if n.starts_with("\r\n") => "\r\n",
                    n if n.starts_with('\r') => "\r",
                    _ => "\n",
                };
                let last = rest.lines().last().unwrap_or_default();
                if let Some(new) = array.get_mut(0) {
                    new.decor_mut().set_prefix(format!("{first}{newline}{last}"));
                }
                if let Some(old) = array.get_mut(1) {
                    old.decor_mut().set_prefix(format!("{newline}{rest}"));
                }
            }
            None => {
                if let Some(new) = array.get_mut(0) {
                    new.decor_mut().set_prefix(prefix);
                }
            }
        }
    }
    reformat_multiline(array);
}

/// Remove the item at `index`, its prefix, which holds the comment at the end of the line before, going to what
/// follows it.
fn remove_at(array: &mut Array, index: usize) {
    let prefix = array
        .get(index)
        .and_then(|v| v.decor().prefix())
        .and_then(RawString::as_str)
        .filter(|p| !p.is_empty())
        .map(str::to_owned);
    if let Some(prefix) = prefix {
        match array.get_mut(index + 1) {
            Some(next) => {
                let existing = next.decor().prefix().and_then(RawString::as_str).unwrap_or_default();
                let joined = format!("{prefix}{existing}");
                next.decor_mut().set_prefix(joined);
            }
            None => {
                let existing = array.trailing().as_str().unwrap_or_default();
                let joined = format!("{prefix}{existing}");
                array.set_trailing(joined);
            }
        }
    }
    let _ = array.remove(index);
    reformat_multiline(array);
}

/// A comment in an item's decoration: on a line of its own, or at the end of a line after this whitespace.
struct Comment {
    text: String,
    end_of_line: Option<String>,
}

fn comments(raw: Option<&RawString>) -> Vec<Comment> {
    let mut found = Vec::new();
    let (mut prev_empty, mut prev_comment) = (false, false);
    for line in raw.and_then(RawString::as_str).unwrap_or_default().lines() {
        let trimmed = line.trim();
        if let Some((before, comment)) = line.split_once('#') {
            let end_of_line = if prev_empty || prev_comment {
                None
            } else {
                let spaces = before.len() - before.trim_end().len();
                Some(before[before.len() - spaces..].to_owned())
            };
            found.push(Comment {
                text: format!("#{}", comment.trim_end()),
                end_of_line,
            });
            prev_comment = true;
        } else {
            prev_comment = false;
        }
        prev_empty = trimmed.is_empty();
    }
    found
}

/// Lay the array out one item per line, with a trailing comma, indented as its first item is (four spaces if it's
/// on the bracket's line), each comment kept on its own line or at the end of the line it was on.
fn reformat_multiline(array: &mut Array) {
    // Without a trailing comma, comments after the last item are in its suffix; with one, they follow the comma.
    let commented_suffix = if array.trailing_comma() {
        None
    } else {
        array
            .iter()
            .last()
            .and_then(|last| last.decor().suffix())
            .and_then(RawString::as_str)
            .filter(|suffix| suffix.contains('#'))
            .map(str::to_owned)
    };
    if let Some(suffix) = commented_suffix {
        if let Some(last) = array.iter_mut().last() {
            last.decor_mut().set_suffix("");
        }
        let trailing = array.trailing().as_str().unwrap_or_default();
        let joined = format!("{suffix}{trailing}");
        array.set_trailing(joined);
    }

    let indent = array
        .iter()
        .next()
        .and_then(|first| first.decor().prefix())
        .and_then(RawString::as_str)
        // Only from an item on a line of its own: uv also takes the space after the bracket of a one-line array, and
        // so indents by one space after `uv remove` of its first item.
        .filter(|p| p.contains('\n'))
        .and_then(|p| p.lines().last())
        .map(|line| line.split_once('#').map_or(line, |(before, _)| before))
        .filter(|indent| !indent.is_empty())
        .unwrap_or("    ");
    let line_start = format!("\n{indent}");

    let place = |found: Vec<Comment>, out: &mut String| {
        for comment in found {
            match &comment.end_of_line {
                None => out.push_str(&line_start),
                Some(spaces) => out.push_str(spaces),
            }
            out.push_str(&comment.text);
        }
    };
    for item in array.iter_mut() {
        let decor = item.decor_mut();
        let mut prefix = String::new();
        let mut found = comments(decor.prefix());
        found.extend(comments(decor.suffix()));
        place(found, &mut prefix);
        prefix.push_str(&line_start);
        decor.set_prefix(prefix);
        decor.set_suffix("");
    }
    let mut trailing = String::new();
    place(comments(Some(array.trailing())), &mut trailing);
    if !trailing.is_empty() || !array.is_empty() {
        trailing.push('\n');
    }
    array.set_trailing(trailing);
    array.set_trailing_comma(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| (*k).to_owned()).collect()
    }

    const TEXT: &str = "a = 1\n[project]\ncollections = [\"a.a\"]\n";

    #[test]
    fn check_refuses_another_change() {
        let mut doc = Document::new(TEXT).unwrap();
        doc.doc["a"] = toml_edit::value(2);
        let err = doc.check(&path(&["project", "collections"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "editing project.collections would change more than it should"
        );
        assert_eq!(doc.text(), TEXT);
    }

    #[test]
    fn check_refuses_unreadable_text() {
        let mut doc = Document::new(TEXT).unwrap();
        doc.doc["a"].as_value_mut().unwrap().decor_mut().set_suffix(" x");
        let err = doc.check(&path(&["a"])).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("editing a would leave the file unreadable: "),
            "{err}"
        );
        assert_eq!(doc.text(), TEXT);
    }

    #[test]
    fn refused_edit_is_undone() {
        let mut doc = Document::new(TEXT).unwrap();
        let err = doc.edit(|doc| {
            doc.doc["a"] = toml_edit::value(2);
            doc.check(&path(&["a"]))
        });
        assert!(err.is_err());
        doc.insert(&path(&["project", "collections"]), 1, "b.b").unwrap();
        assert_eq!(doc.document()["a"].as_integer(), Some(1));
        assert_eq!(
            doc.text(),
            "a = 1\n[project]\ncollections = [\n    \"a.a\",\n    \"b.b\",\n]\n"
        );
    }
}
