// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! YAML read with libyaml's parser, as Ansible reads it through PyYAML, into plain values.
//!
//! Scalars are strings. This doesn't follow YAML 1.1's or PyYAML's rules for other types, to keep it simple, except
//! for a plain, untagged scalar that's a boolean as they spell one (`yes`, `no`, `on`, `off`, `true` or `false`), null
//! (`~`, `null` or empty), or an integer or float whose Python `str()` is the original text, such as `42` or `2.9`.
//! Booleans are kept because Ansible's docs rely on them everywhere, and null and numbers where converting them loses
//! nothing. Anything else stays the text its author wrote, so `2.10`, `0644` and `2021-01-01` stay strings.

use std::collections::HashMap;

use libyaml_safer::{EventData, Mark, Parser, ScalarStyle};

/// The deepest a document may nest, so building it, and turning it into another language's values, can't run out of
/// stack.
pub const MAX_DEPTH: usize = 256;

/// A value as read. Each node with an anchor is in `Document::anchors`, and it, and every alias to it, is an `Alias`
/// in the tree, so an alias is shared rather than copied, as PyYAML shares it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// An integer too large for `i64`, as its digits.
    BigInt(String),
    Float(f64),
    String(String),
    Sequence(Vec<Value>),
    /// The entries in the order written. A key given twice is kept once, where it was first, with the last value.
    Mapping(Vec<(Value, Value)>),
    Alias(usize),
}

/// Something worth telling the reader that didn't stop the document being read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// What it is, `duplicate_key`.
    pub kind: &'static str,
    pub message: String,
    /// Where it is, from 1.
    pub line: u64,
    pub column: u64,
}

/// A document as read, and what was found worth warning about.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub root: Value,
    /// Each anchored node, by the id its `Value::Alias` gives.
    pub anchors: Vec<Value>,
    pub warnings: Vec<Warning>,
}

impl Document {
    /// `value` with an alias followed to what it names.
    pub fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        match value {
            Value::Alias(id) => &self.anchors[*id],
            other => other,
        }
    }
}

/// Text that isn't YAML Ansible could read, with the message to show for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlError {
    pub message: String,
    /// Where it is, from 1, when the parser said.
    pub line: Option<u64>,
    pub column: Option<u64>,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, "{} (at line {line}, column {column})", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for YamlError {}

impl YamlError {
    fn at(message: impl Into<String>, mark: Mark) -> Self {
        YamlError {
            message: message.into(),
            line: Some(mark.line + 1),
            column: Some(mark.column + 1),
        }
    }
}

/// `data` as text, or an error saying where it stops being UTF-8.
pub fn decode(data: &[u8]) -> Result<&str, YamlError> {
    std::str::from_utf8(data).map_err(|e| {
        let before = String::from_utf8_lossy(&data[..e.valid_up_to()]);
        let line = before.matches('\n').count() as u64 + 1;
        let column = before.rsplit('\n').next().unwrap_or(&before).chars().count() as u64 + 1;
        YamlError {
            message: "not UTF-8".to_owned(),
            line: Some(line),
            column: Some(column),
        }
    })
}

/// Read a single YAML document. An empty stream is `Value::Null`, and a second document is an error, as PyYAML's
/// `load` gives.
pub fn loads(text: &str) -> Result<Document, YamlError> {
    Builder::new(None).run(text).map(|(document, _)| document)
}

/// Read a single YAML document as PyYAML's `BaseLoader` does, as ansible-core reads `meta/runtime.yml`: every scalar is
/// the string written, tags and all, so `yes`, `~` and `2.15` are `"yes"`, `"~"` and `"2.15"`, and a `<<` key isn't
/// merged. An empty stream is still `Value::Null`.
pub fn loads_untyped(text: &str) -> Result<Document, YamlError> {
    let mut builder = Builder::new(None);
    builder.typed = false;
    builder.run(text).map(|(document, _)| document)
}

/// Read only the top-level keys `keys` of a document whose root is a mapping, building nothing else, for a caller
/// that wants a few fields of a large document. None if the root isn't a mapping.
pub fn select(text: &str, keys: &[&str]) -> Result<Option<Document>, YamlError> {
    Builder::new(Some(keys))
        .run(text)
        .map(|(document, mapping)| mapping.then_some(document))
}

/// A collection being built. A skipped one, outside the keys `select` wants, builds nothing in it but anchored nodes,
/// which a wanted key may alias.
enum Frame {
    Sequence {
        items: Vec<Value>,
        anchor: Option<String>,
        skip: bool,
    },
    Mapping {
        entries: Vec<(Value, Value)>,
        /// The key read, where it was, and whether it's the plain merge key `<<`.
        key: Option<(Value, Mark, bool)>,
        anchor: Option<String>,
        skip: bool,
        keys: HashMap<Key, usize>,
        merges: Vec<Value>,
    },
}

struct Builder<'a> {
    select: Option<&'a [&'a str]>,
    /// Whether scalars are resolved and `<<` merged, or every scalar kept as its text.
    typed: bool,
    stack: Vec<Frame>,
    anchors: Vec<Value>,
    names: HashMap<String, usize>,
    warnings: Vec<Warning>,
    /// Nodes still allowed, so merge keys copying entries can't build far more than the text holds.
    budget: usize,
}

impl<'a> Builder<'a> {
    fn new(select: Option<&'a [&'a str]>) -> Self {
        Builder {
            select,
            typed: true,
            stack: Vec::new(),
            anchors: Vec::new(),
            names: HashMap::new(),
            warnings: Vec::new(),
            budget: 0,
        }
    }

    fn run(mut self, text: &str) -> Result<(Document, bool), YamlError> {
        // libyaml-safer's scanner can panic at the end of input without a final newline, which adding one avoids.
        let owned;
        let (text, added) = if text.ends_with('\n') {
            (text, false)
        } else {
            owned = format!("{text}\n");
            (owned.as_str(), true)
        };
        self.budget = 2 * text.len() + 1024;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.parse(text, added))) {
            Ok(result) => result,
            Err(_) => Err(YamlError {
                message: "the YAML parser failed on this text".to_owned(),
                line: None,
                column: None,
            }),
        }
    }

    /// Read `text`. With `added`, its final newline wasn't in what was given, so a block scalar running to the end
    /// loses the line break it gave, as without it there was none.
    fn parse(mut self, text: &str, added: bool) -> Result<(Document, bool), YamlError> {
        let mut input = text.as_bytes();
        let mut parser = Parser::new();
        parser.set_input_string(&mut input);
        let mut root = None;
        let mut documents = 0;
        let mut root_mapping = false;
        loop {
            let event = parser.parse().map_err(parse_error)?;
            let mark = event.start_mark;
            let mut merge_key = false;
            let built = match event.data {
                EventData::StreamStart { .. } | EventData::DocumentEnd { .. } => continue,
                EventData::StreamEnd => break,
                EventData::DocumentStart { .. } => {
                    documents += 1;
                    if documents > 1 {
                        return Err(YamlError::at("expected a single document in the stream", mark));
                    }
                    continue;
                }
                EventData::Alias { anchor } => match self.names.get(&anchor) {
                    Some(id) => Value::Alias(*id),
                    None => return Err(YamlError::at(format!("found undefined alias '{anchor}'"), mark)),
                },
                EventData::Scalar {
                    anchor,
                    tag,
                    mut value,
                    style,
                    ..
                } => {
                    if added
                        && matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                        && event.end_mark.index as usize >= text.len()
                        && value.ends_with('\n')
                    {
                        value.pop();
                    }
                    merge_key = self.typed && style == ScalarStyle::Plain && tag.is_none() && value == "<<";
                    let built = if self.skipping() && anchor.is_none() {
                        Value::Null
                    } else {
                        self.spend(mark)?;
                        if self.typed {
                            scalar(value, style, tag)
                        } else {
                            Value::String(value)
                        }
                    };
                    self.anchored(anchor, built)
                }
                EventData::SequenceStart { anchor, .. } => {
                    self.open(mark)?;
                    let skip = self.skipping() && anchor.is_none();
                    self.stack.push(Frame::Sequence {
                        items: Vec::new(),
                        anchor,
                        skip,
                    });
                    continue;
                }
                EventData::MappingStart { anchor, .. } => {
                    self.open(mark)?;
                    root_mapping |= self.stack.is_empty();
                    let skip = self.skipping() && anchor.is_none();
                    self.stack.push(Frame::Mapping {
                        entries: Vec::new(),
                        key: None,
                        anchor,
                        skip,
                        keys: HashMap::new(),
                        merges: Vec::new(),
                    });
                    continue;
                }
                EventData::SequenceEnd | EventData::MappingEnd => self.close(mark)?,
            };
            if self.stack.is_empty() {
                root = Some(built);
            } else {
                self.add(built, mark, merge_key)?;
            }
        }
        let document = Document {
            root: root.unwrap_or(Value::Null),
            anchors: self.anchors,
            warnings: self.warnings,
        };
        Ok((document, root_mapping))
    }

    /// Whether what's being read now is outside what `select` wants.
    fn skipping(&self) -> bool {
        match self.stack.last() {
            None => false,
            Some(Frame::Sequence { skip, .. }) => *skip,
            Some(Frame::Mapping { skip, key, .. }) => {
                if *skip {
                    return true;
                }
                // At the top of a selection, only the wanted keys' values are built.
                match (self.select, self.stack.len(), key) {
                    (Some(wanted), 1, Some((Value::String(k), _, _))) => !wanted.contains(&k.as_str()),
                    (Some(_), 1, Some(_)) => true,
                    _ => false,
                }
            }
        }
    }

    fn spend(&mut self, mark: Mark) -> Result<(), YamlError> {
        if self.budget == 0 {
            return Err(YamlError::at("the document builds far more than its text holds", mark));
        }
        self.budget -= 1;
        Ok(())
    }

    fn open(&mut self, mark: Mark) -> Result<(), YamlError> {
        if self.stack.len() >= MAX_DEPTH {
            return Err(YamlError::at(
                format!("the document nests deeper than {MAX_DEPTH}"),
                mark,
            ));
        }
        self.spend(mark)
    }

    fn anchored(&mut self, anchor: Option<String>, value: Value) -> Value {
        match anchor {
            None => value,
            Some(name) => {
                self.names.insert(name, self.anchors.len());
                self.shared(value)
            }
        }
    }

    /// `value` as an anchor, and the alias to it.
    fn shared(&mut self, value: Value) -> Value {
        self.anchors.push(value);
        Value::Alias(self.anchors.len() - 1)
    }

    fn close(&mut self, mark: Mark) -> Result<Value, YamlError> {
        let frame = self.stack.pop().expect("the parser balances its events");
        match frame {
            Frame::Sequence { items, anchor, skip } => {
                let value = if skip { Value::Null } else { Value::Sequence(items) };
                Ok(self.anchored(anchor, value))
            }
            Frame::Mapping {
                mut entries,
                anchor,
                skip,
                merges,
                ..
            } => {
                if !merges.is_empty() {
                    entries = self.merge(merges, entries, mark)?;
                }
                let value = if skip { Value::Null } else { Value::Mapping(entries) };
                Ok(self.anchored(anchor, value))
            }
        }
    }

    /// The entries of a mapping with `<<` merge keys, as PyYAML's `flatten_mapping` gives them: the merged mappings'
    /// entries, the last of a list first, then the mapping's own, each later value replacing an earlier one where it
    /// stands. A merged collection is shared, as PyYAML shares it, so a merge copies keys, not values.
    fn merge(
        &mut self,
        merges: Vec<Value>,
        own: Vec<(Value, Value)>,
        mark: Mark,
    ) -> Result<Vec<(Value, Value)>, YamlError> {
        let mut sources = Vec::new();
        for merge in merges {
            let items = match merge {
                Value::Alias(id) if matches!(self.anchors[id], Value::Sequence(_)) => match &self.anchors[id] {
                    Value::Sequence(items) => items.clone(),
                    _ => unreachable!(),
                },
                Value::Sequence(items) => items,
                single => vec![single],
            };
            let mut list = Vec::new();
            for item in items {
                list.push(self.entries_of(item, mark)?);
            }
            list.reverse();
            sources.extend(list);
        }
        sources.push(own);
        let mut merged: Vec<(Value, Value)> = Vec::new();
        let mut seen: HashMap<Key, usize> = HashMap::new();
        for (key, value) in sources.into_iter().flatten() {
            let id = Key::of(self.follow(&key)).expect("a mapping's keys were checked when it was read");
            match seen.get(&id) {
                Some(&at) => merged[at].1 = value,
                None => {
                    self.spend(mark)?;
                    seen.insert(id, merged.len());
                    merged.push((key, value));
                }
            }
        }
        Ok(merged)
    }

    /// The entries of a mapping a merge key names. An anchored one's collections become anchors of their own first, so
    /// its entries can be copied without copying what they hold.
    fn entries_of(&mut self, value: Value, mark: Mark) -> Result<Vec<(Value, Value)>, YamlError> {
        match value {
            Value::Mapping(entries) => Ok(entries),
            Value::Alias(id) if matches!(self.anchors[id], Value::Mapping(_)) => {
                let Value::Mapping(mut entries) = std::mem::replace(&mut self.anchors[id], Value::Null) else {
                    unreachable!()
                };
                for (_, held) in &mut entries {
                    if matches!(held, Value::Sequence(_) | Value::Mapping(_)) {
                        let shared = std::mem::replace(held, Value::Null);
                        *held = self.shared(shared);
                    }
                }
                self.anchors[id] = Value::Mapping(entries.clone());
                Ok(entries)
            }
            _ => Err(YamlError::at(
                "a merge key's value must be a mapping or a list of them",
                mark,
            )),
        }
    }

    fn follow<'v>(&'v self, value: &'v Value) -> &'v Value {
        match value {
            Value::Alias(id) => &self.anchors[*id],
            other => other,
        }
    }

    fn add(&mut self, value: Value, mark: Mark, merge_key: bool) -> Result<(), YamlError> {
        let select_top = self.select.is_some() && self.stack.len() == 1;
        let frame = self.stack.last_mut().expect("add is only called inside a collection");
        match frame {
            Frame::Sequence { items, skip, .. } => {
                if !*skip {
                    items.push(value);
                }
            }
            Frame::Mapping {
                entries,
                key,
                skip,
                keys,
                merges,
                ..
            } => match key.take() {
                None => *key = Some((value, mark, merge_key)),
                Some((k, key_mark, is_merge)) => {
                    if *skip {
                        return Ok(());
                    }
                    if is_merge && !select_top {
                        merges.push(value);
                        return Ok(());
                    }
                    let resolved = match &k {
                        Value::Alias(id) => &self.anchors[*id],
                        other => other,
                    };
                    let Some(id) = Key::of(resolved) else {
                        return Err(YamlError::at("found unhashable key", key_mark));
                    };
                    if select_top && !matches!(&id, Key::Str(s) if self.select.is_some_and(|w| w.contains(&s.as_str())))
                    {
                        return Ok(());
                    }
                    match keys.get(&id) {
                        Some(&at) => {
                            self.warnings.push(Warning {
                                kind: "duplicate_key",
                                message: format!("found duplicate mapping key {}", id.shown()),
                                line: key_mark.line + 1,
                                column: key_mark.column + 1,
                            });
                            entries[at].1 = value;
                        }
                        None => {
                            keys.insert(id, entries.len());
                            entries.push((k, value));
                        }
                    }
                }
            },
        }
        Ok(())
    }
}

/// A key as Python's `dict` tells keys apart, so a key given twice is found as PyYAML would find it: `true`, `1` and
/// `1.0` are the same key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Null,
    Int(i64),
    BigInt(String),
    Float(u64),
    Str(String),
}

impl Key {
    fn of(value: &Value) -> Option<Key> {
        Some(match value {
            Value::Null => Key::Null,
            Value::Bool(b) => Key::Int(i64::from(*b)),
            Value::Int(n) => Key::Int(*n),
            Value::BigInt(digits) => Key::BigInt(digits.clone()),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Key::Int(*f as i64),
            Value::Float(f) => Key::Float(f.to_bits()),
            Value::String(s) => Key::Str(s.clone()),
            Value::Sequence(_) | Value::Mapping(_) | Value::Alias(_) => return None,
        })
    }

    fn shown(&self) -> String {
        match self {
            Key::Null => "None".to_owned(),
            Key::Int(n) => n.to_string(),
            Key::BigInt(digits) => digits.clone(),
            Key::Float(bits) => f64::from_bits(*bits).to_string(),
            Key::Str(s) => format!("'{s}'"),
        }
    }
}

fn parse_error(err: libyaml_safer::Error) -> YamlError {
    let message = match err.context() {
        Some(context) if !context.is_empty() => format!("{} {context}", err.problem()),
        _ => err.problem().to_owned(),
    };
    match err.problem_mark() {
        Some(mark) => YamlError::at(message, mark),
        None => YamlError {
            message,
            line: None,
            column: None,
        },
    }
}

/// A scalar as a value: a plain, untagged one resolved as the module describes, anything else a string as written.
/// A tag with nothing after it was meant as text, as `sample: !abc:server.tld` is, so it's the tag's text.
fn scalar(value: String, style: ScalarStyle, tag: Option<String>) -> Value {
    if let Some(tag) = tag {
        if value.is_empty() && style == ScalarStyle::Plain && !tag.starts_with("tag:yaml.org,2002:") {
            return Value::String(tag);
        }
        return Value::String(value);
    }
    if style != ScalarStyle::Plain {
        return Value::String(value);
    }
    resolve(value)
}

/// A plain scalar's value, as the module describes.
pub fn resolve(text: String) -> Value {
    match text.as_str() {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => return Value::Bool(true),
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => return Value::Bool(false),
        _ => {}
    }
    if is_decimal_int(&text) {
        return match text.parse::<i64>() {
            Ok(n) => Value::Int(n),
            Err(_) => Value::BigInt(text),
        };
    }
    if let Some(f) = float_as_written(&text) {
        return Value::Float(f);
    }
    Value::String(text)
}

/// `0`, or digits not starting with `0`, with an optional `-`, so the integer is written back the same: `-0`, `+1`,
/// `0644`, `0x1F`, `1_000` and `11:33` aren't.
fn is_decimal_int(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0"
}

/// The float `text` is, if it's digits, a `.` and digits that Python's `str()` gives back exactly, such as `2.9` or
/// `1.0`, but not `2.10`, `1.50`, `.5`, `1e3` or `1.0e3`.
fn float_as_written(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = unsigned.split_once('.')?;
    if whole.is_empty()
        || fraction.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (whole.len() > 1 && whole.starts_with('0'))
    {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    // Python writes a float with an exponent outside this range, and Rust never does.
    if value != 0.0 && !(1e-4..1e16).contains(&value.abs()) {
        return None;
    }
    let mut written = value.to_string();
    if !written.contains('.') {
        written.push_str(".0");
    }
    (written == text).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_what_writes_back() {
        assert_eq!(resolve("yes".into()), Value::Bool(true));
        assert_eq!(resolve("Off".into()), Value::Bool(false));
        assert_eq!(resolve("y".into()), Value::String("y".into()));
        assert_eq!(resolve("~".into()), Value::Null);
        assert_eq!(resolve("".into()), Value::Null);
        assert_eq!(resolve("42".into()), Value::Int(42));
        assert_eq!(resolve("-7".into()), Value::Int(-7));
        assert_eq!(
            resolve("99999999999999999999".into()),
            Value::BigInt("99999999999999999999".into())
        );
        assert_eq!(resolve("2.9".into()), Value::Float(2.9));
        assert_eq!(resolve("1.0".into()), Value::Float(1.0));
        assert_eq!(resolve("-0.5".into()), Value::Float(-0.5));
        for kept in [
            "2.10",
            "1.50",
            "0644",
            "-0",
            "+1",
            "0x1F",
            "1_000",
            "11:33:47",
            "1e3",
            "1.0e+3",
            ".5",
            "5.",
            ".inf",
            "2021-01-01",
            "0.00001",
            "1.0.0",
        ] {
            assert_eq!(resolve(kept.into()), Value::String(kept.into()), "{kept}");
        }
    }

    fn root(text: &str) -> Value {
        loads(text).unwrap().root
    }

    fn s(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    #[test]
    fn scalars_by_style_and_tag() {
        let doc = root("a: \"yes\"\nb: !!str 1\nc: !foo yes\nd: !abc:server.tld\ne: |\n  2.9\n");
        assert_eq!(
            doc,
            Value::Mapping(vec![
                (s("a"), s("yes")),
                (s("b"), s("1")),
                (s("c"), s("yes")),
                (s("d"), s("!abc:server.tld")),
                (s("e"), s("2.9\n")),
            ])
        );
    }

    #[test]
    fn duplicate_key_keeps_its_place_with_the_last_value() {
        let doc = loads("a: 1\nb: 2\na: 3\n1: x\ntrue: y\n").unwrap();
        assert_eq!(
            doc.root,
            Value::Mapping(vec![
                (s("a"), Value::Int(3)),
                (s("b"), Value::Int(2)),
                (Value::Int(1), s("y"))
            ])
        );
        let found: Vec<_> = doc
            .warnings
            .iter()
            .map(|w| (w.message.as_str(), w.line, w.column))
            .collect();
        assert_eq!(
            found,
            [
                ("found duplicate mapping key 'a'", 3, 1),
                ("found duplicate mapping key 1", 5, 1)
            ]
        );
    }

    #[test]
    fn aliases_are_shared() {
        let doc = loads("a: &x [1, 2]\nb: *x\n").unwrap();
        assert_eq!(
            doc.root,
            Value::Mapping(vec![(s("a"), Value::Alias(0)), (s("b"), Value::Alias(0))])
        );
        assert_eq!(doc.anchors, [Value::Sequence(vec![Value::Int(1), Value::Int(2)])]);
        assert!(loads("a: *nope\n").unwrap_err().message.contains("undefined alias"));
    }

    #[test]
    fn merge_keys() {
        let text = "base: &b {x: 1, y: 2}\nmore: &m {y: 3, z: 4}\nc:\n  <<: [*b, *m]\n  x: 9\nd:\n  \"<<\": 1\n";
        let doc = loads(text).unwrap();
        let Value::Mapping(entries) = &doc.root else { panic!() };
        assert_eq!(
            entries[2].1,
            Value::Mapping(vec![
                (s("y"), Value::Int(2)),
                (s("z"), Value::Int(4)),
                (s("x"), Value::Int(9))
            ])
        );
        assert_eq!(entries[3].1, Value::Mapping(vec![(s("<<"), Value::Int(1))]));
    }

    #[test]
    fn select_builds_only_the_keys_asked_for() {
        let text = "short_description: Hi\noptions: &o {a: {b: [1]}}\nversion_added: 2.10\nalias: *o\n";
        let doc = select(text, &["short_description", "version_added", "alias"])
            .unwrap()
            .unwrap();
        assert_eq!(
            doc.root,
            Value::Mapping(vec![
                (s("short_description"), s("Hi")),
                (s("version_added"), s("2.10")),
                (s("alias"), Value::Alias(0)),
            ])
        );
        let Value::Mapping(options) = &doc.anchors[0] else {
            panic!()
        };
        assert_eq!(options.len(), 1);
        assert_eq!(select("- a\n", &["a"]).unwrap(), None);
    }

    #[test]
    fn documents() {
        assert_eq!(root(""), Value::Null);
        assert_eq!(root("# only a comment\n"), Value::Null);
        assert!(
            loads("a: 1\n---\nb: 2\n")
                .unwrap_err()
                .message
                .contains("single document")
        );
        let err = loads("a: [").unwrap_err();
        assert_eq!((err.line, err.column), (Some(2), Some(1)));
        assert!(err.to_string().ends_with("(at line 2, column 1)"), "{err}");
    }

    #[test]
    fn block_scalar_at_the_end_without_a_newline() {
        assert_eq!(root("a: |\n  x\n  y"), Value::Mapping(vec![(s("a"), s("x\ny"))]));
        assert_eq!(root("a: |\n  x\n"), Value::Mapping(vec![(s("a"), s("x\n"))]));
        assert_eq!(root("a: >\n  x\n  y"), Value::Mapping(vec![(s("a"), s("x y"))]));
        assert_eq!(root("a: |-\n  x"), Value::Mapping(vec![(s("a"), s("x"))]));
        assert_eq!(root("a: |+\n  x"), Value::Mapping(vec![(s("a"), s("x"))]));
        assert_eq!(
            root("a: |\n  x\nb: y"),
            Value::Mapping(vec![(s("a"), s("x\n")), (s("b"), s("y"))])
        );
    }

    #[test]
    fn no_final_newline() {
        // libyaml-safer 0.3.0 panics on this without the newline loads adds.
        let text = "DOCUMENTATION:\n  options:\n    _input:\n      item_list:\n        - goofy\n        - donald";
        assert!(loads(text).is_ok());
    }

    #[test]
    fn limits() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(loads(&deep).unwrap_err().message.contains("nests deeper"));
        assert!(loads(&format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH))).is_ok());
        assert!(
            loads("a: [1]\nb: {<<: [1]}\n")
                .unwrap_err()
                .message
                .contains("merge key")
        );
        assert!(loads("? [1]\n: x\n").unwrap_err().message.contains("unhashable"));
    }

    fn top<'a>(doc: &'a Document, key: &str) -> &'a Value {
        let Value::Mapping(entries) = &doc.root else {
            panic!("not a mapping")
        };
        doc.resolve(&entries.iter().find(|(k, _)| *k == s(key)).expect("no such key").1)
    }

    #[test]
    fn merges_share_rather_than_copy() {
        // Each level merges the one before twice. Keys are merged once each, so the last holds a and k1 to k39.
        let mut text = String::from("l0: &l0 {a: [1, 2, 3, 4, 5, 6, 7, 8]}\n");
        for n in 1..40 {
            text.push_str(&format!("l{n}: &l{n} {{<<: [*l{0}, *l{0}], k{n}: [1, 2]}}\n", n - 1));
        }
        let doc = loads(&text).unwrap();
        assert!(matches!(top(&doc, "l39"), Value::Mapping(entries) if entries.len() == 40));
        // A large mapping merged into many is shared by them all.
        let mut text = format!("big: &b {{v: [{}]}}\nall:\n", ["1"; 1000].join(", "));
        text.push_str(&"  - {<<: *b}\n".repeat(5000));
        let doc = loads(&text).unwrap();
        let Value::Sequence(all) = top(&doc, "all") else {
            panic!()
        };
        let Value::Mapping(first) = &all[0] else { panic!() };
        assert!(matches!(first[0].1, Value::Alias(_)));
        assert!(all.iter().all(|m| *m == all[0]));
        // Each line merging every key before it copies more each time, so the budget stops it.
        let mut text = String::from("k0: &k0 {a0: 0}\n");
        for n in 1..3000 {
            text.push_str(&format!("k{n}: &k{n} {{<<: *k{}, a{n}: 0}}\n", n - 1));
        }
        assert!(
            loads(&text)
                .unwrap_err()
                .message
                .contains("far more than its text holds")
        );
    }

    #[test]
    fn merge_order() {
        // As PyYAML orders them: the list's last mapping first, the first winning, then the mapping's own.
        let doc = loads("p: &p {p: 1, x: 1}\nq: &q {q: 2, x: 2}\nr: {<<: [*p, *q], o: 3}\n").unwrap();
        assert_eq!(
            *top(&doc, "r"),
            Value::Mapping(vec![
                (s("q"), Value::Int(2)),
                (s("x"), Value::Int(1)),
                (s("p"), Value::Int(1)),
                (s("o"), Value::Int(3))
            ])
        );
    }

    #[test]
    fn untyped_as_base_loader() {
        let text = "a: true\nb: yes\nc: ~\nd:\ne: 2.15\nf: !!int 5\ng: &x {p: 1}\nh: {<<: *x, q: 2}\n";
        let doc = loads_untyped(text).unwrap();
        let Value::Mapping(entries) = &doc.root else { panic!() };
        let scalars: Vec<_> = entries[..6].iter().map(|(_, v)| v.clone()).collect();
        assert_eq!(scalars, [s("true"), s("yes"), s("~"), s(""), s("2.15"), s("5")]);
        // A merge key stays a key, holding the mapping.
        let Value::Mapping(h) = &entries[7].1 else { panic!() };
        assert_eq!(h[0], (s("<<"), Value::Alias(0)));
        assert_eq!(loads_untyped("").unwrap().root, Value::Null);
    }

    #[test]
    fn decode_says_where() {
        let err = decode(b"a: 1\nb: caf\xe9\n").unwrap_err();
        assert_eq!(
            (err.message.as_str(), err.line, err.column),
            ("not UTF-8", Some(2), Some(7))
        );
        assert_eq!(decode("é: x".as_bytes()).unwrap(), "é: x");
    }
}
