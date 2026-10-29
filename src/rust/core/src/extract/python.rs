// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! What extraction reads from a collection's Python files, from the syntax tree alone: nothing is imported or run.

use std::collections::{HashMap, HashSet};

use ruff_python_ast::{ExceptHandler, Expr, ModModule, Number, Operator, Stmt, UnaryOp};

use crate::yaml::Value;

/// The file parsed as Python 3, or None if it isn't, as Python 2 often isn't. Bytes that aren't UTF-8, such as a
/// comment in another encoding, are replaced first, as Python 3.15 refuses them anywhere in a file. Line endings are
/// read as Python reads them, `\r\n` and `\r` as `\n`, so a string spanning lines has the same value it does there.
pub(super) fn parse(data: &[u8]) -> Option<ModModule> {
    let mut text = match data.strip_prefix(b"\xef\xbb\xbf") {
        Some(data) => String::from_utf8_lossy(data).into_owned(),
        None if is_latin1(data) => data.iter().map(|&b| char::from(b)).collect(),
        None => String::from_utf8_lossy(data).into_owned(),
    };
    if text.contains('\r') {
        text = text.replace("\r\n", "\n").replace('\r', "\n");
    }
    ruff_python_parser::parse_module(&text)
        .ok()
        .map(|parsed| parsed.into_syntax())
}

/// Whether the file declares a Latin-1 encoding, as PEP 263 has it: `coding[:=]name` in a comment on the first
/// line, or on the second if the first is blank or a comment.
// shortcut: only Latin-1 is decoded, as the latest collections declare no other encoding than UTF-8; others, such as
// cp1252, need an encoding crate, so their bytes outside ASCII are replaced instead. Upgrade if one turns up.
fn is_latin1(data: &[u8]) -> bool {
    let mut lines = data.split(|&b| b == b'\n').take(2);
    let declared = |line: &[u8]| {
        let line = String::from_utf8_lossy(line);
        let comment = line.trim_start_matches([' ', '\t', '\x0c']);
        let rest = comment.strip_prefix('#')?;
        let at = rest.find("coding")?;
        let rest = rest[at + "coding".len()..].strip_prefix([':', '='])?;
        let name: String = rest
            .trim_start_matches([' ', '\t'])
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        Some(name.to_lowercase().replace('_', "-"))
    };
    let first = lines.next().unwrap_or_default();
    let name = declared(first).or_else(|| {
        let blank_or_comment = {
            let text = String::from_utf8_lossy(first);
            let text = text.trim();
            text.is_empty() || text.starts_with('#')
        };
        if blank_or_comment {
            lines.next().and_then(declared)
        } else {
            None
        }
    });
    matches!(
        name.as_deref(),
        Some("latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "l1" | "iso-latin-1" | "cp819")
    )
}

fn string_constant(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str()),
        _ => None,
    }
}

/// The string constants a class body leaves its names set to, as Python sets them: by name, in the order first
/// assigned, the last assignment winning, and the names last assigned something else. Anything but a constant
/// would need evaluating, which extraction never does.
pub(super) fn string_assignments(body: &[Stmt]) -> (Vec<(String, String)>, HashSet<String>) {
    let mut constants: Vec<(String, String)> = Vec::new();
    let mut other = HashSet::new();
    for stmt in body {
        let (targets, value) = match stmt {
            Stmt::Assign(a) => (a.targets.as_slice(), &*a.value),
            Stmt::AnnAssign(a) => match &a.value {
                Some(value) => (std::slice::from_ref(&*a.target), &**value),
                None => continue,
            },
            _ => continue,
        };
        for target in targets {
            let Expr::Name(name) = target else {
                continue;
            };
            let id = name.id.as_str();
            match string_constant(value) {
                Some(text) => {
                    match constants.iter_mut().find(|(k, _)| k == id) {
                        Some(entry) => entry.1 = text.to_owned(),
                        None => constants.push((id.to_owned(), text.to_owned())),
                    }
                    other.remove(id);
                }
                None => {
                    constants.retain(|(k, _)| k != id);
                    other.insert(id.to_owned());
                }
            }
        }
    }
    (constants, other)
}

/// A docs constant as ansible-core takes it from a Python file.
pub(super) enum DocValue {
    /// A string or bytes, YAML for `DOCUMENTATION` and `RETURN`, and the text itself for `EXAMPLES`.
    Text(String),
    /// A dict literal, as `ast.literal_eval` gives it.
    Literal(Value),
}

/// Each `DOCUMENTATION`, `EXAMPLES` and `RETURN` assignment at the top of `tree`, in order, as ansible-core's
/// `read_docstring_from_python_file` reads them: from a plain `=` assignment (not an annotated one), to every name
/// it assigns, a dict literal as `ast.literal_eval` gives it, and a string or bytes as its text. Every assignment is
/// given, for the caller to let a later one replace an earlier one. None for a value ansible-core can't read: an
/// expression, such as `"a" + "b"`, or a number for `DOCUMENTATION` or `RETURN`.
pub(super) fn doc_assignments(tree: &ModModule) -> Vec<(&'static str, Option<DocValue>)> {
    let mut found = Vec::new();
    for stmt in &tree.body {
        let Stmt::Assign(assign) = stmt else {
            continue;
        };
        for target in &assign.targets {
            let Expr::Name(name) = target else {
                continue;
            };
            let Some(key) = ["DOCUMENTATION", "EXAMPLES", "RETURN"]
                .into_iter()
                .find(|k| *k == name.id.as_str())
            else {
                continue;
            };
            let value = match &*assign.value {
                Expr::Dict(_) => literal(&assign.value).map(DocValue::Literal),
                Expr::StringLiteral(s) => Some(DocValue::Text(s.value.to_str().to_owned())),
                Expr::BytesLiteral(b) => Some(DocValue::Text(
                    String::from_utf8_lossy(&b.value.bytes().collect::<Vec<u8>>()).into_owned(),
                )),
                // EXAMPLES is taken as the text of any constant; the others are YAML, which a number isn't.
                Expr::NumberLiteral(n) if key == "EXAMPLES" => match &n.value {
                    Number::Int(i) => Some(DocValue::Text(i.to_string())),
                    Number::Float(f) => Some(DocValue::Text(f.to_string())),
                    Number::Complex { .. } => None,
                },
                Expr::BooleanLiteral(b) if key == "EXAMPLES" => {
                    Some(DocValue::Text(if b.value { "True" } else { "False" }.to_owned()))
                }
                Expr::NoneLiteral(_) if key == "EXAMPLES" => Some(DocValue::Text("None".to_owned())),
                _ => None,
            };
            found.push((key, value));
        }
    }
    found
}

/// A literal as Python's `ast.literal_eval` gives it, or None for anything it refuses, or JSON can't hold, such as
/// a complex number. Bytes become their text.
fn literal(expr: &Expr) -> Option<Value> {
    Some(match expr {
        Expr::StringLiteral(s) => Value::String(s.value.to_str().to_owned()),
        Expr::BytesLiteral(b) => {
            Value::String(String::from_utf8_lossy(&b.value.bytes().collect::<Vec<u8>>()).into_owned())
        }
        Expr::NumberLiteral(n) => number(&n.value)?,
        Expr::BooleanLiteral(b) => Value::Bool(b.value),
        Expr::NoneLiteral(_) => Value::Null,
        Expr::UnaryOp(op) if matches!(op.op, UnaryOp::USub | UnaryOp::UAdd) => {
            let Expr::NumberLiteral(n) = &*op.operand else {
                return None;
            };
            match (op.op, number(&n.value)?) {
                (UnaryOp::USub, Value::Int(i)) => Value::Int(i.checked_neg()?),
                (UnaryOp::USub, Value::BigInt(digits)) => Value::BigInt(format!("-{digits}")),
                (UnaryOp::USub, Value::Float(f)) => Value::Float(-f),
                (_, value) => value,
            }
        }
        Expr::Dict(dict) => Value::Mapping(
            dict.items
                .iter()
                .map(|item| Some((literal(item.key.as_ref()?)?, literal(&item.value)?)))
                .collect::<Option<_>>()?,
        ),
        Expr::List(list) => Value::Sequence(list.elts.iter().map(literal).collect::<Option<_>>()?),
        Expr::Tuple(tuple) => Value::Sequence(tuple.elts.iter().map(literal).collect::<Option<_>>()?),
        Expr::Set(set) => Value::Sequence(set.elts.iter().map(literal).collect::<Option<_>>()?),
        _ => return None,
    })
}

fn number(n: &Number) -> Option<Value> {
    Some(match n {
        Number::Int(i) => i.as_i64().map_or_else(|| Value::BigInt(i.to_string()), Value::Int),
        Number::Float(f) => Value::Float(*f),
        Number::Complex { .. } => return None,
    })
}

/// Whether Python's `str.isupper` is true: there's a cased character, and none is lowercase.
fn is_upper(text: &str) -> bool {
    text.chars().any(|c| c.is_uppercase()) && !text.chars().any(char::is_lowercase)
}

/// A doc fragment's `ModuleDocFragment` string attributes, and whether it may have any that couldn't be read.
///
/// Only uppercase names count: ansible-core looks up `ns.coll.frag.var` as the attribute `VAR`, and plain
/// `ns.coll.frag` as `DOCUMENTATION`. A method can set attributes at runtime (amazon.aws's boto3 builds them all in
/// `__init__`), so a class with one, or with nothing readable, is unresolved.
pub(super) fn fragment_attributes(tree: &ModModule) -> (Vec<(String, String)>, bool) {
    for stmt in &tree.body {
        let Stmt::ClassDef(class) = stmt else {
            continue;
        };
        if class.name.as_str() != "ModuleDocFragment" {
            continue;
        }
        let (constants, other) = string_assignments(&class.body);
        let attributes: Vec<_> = constants.into_iter().filter(|(k, _)| is_upper(k)).collect();
        let has_method = class.body.iter().any(|item| matches!(item, Stmt::FunctionDef(_)));
        let unresolved = attributes.is_empty() || has_method || other.iter().any(|k| is_upper(k));
        return (attributes, unresolved);
    }
    (Vec::new(), true)
}

/// A name's string keys, None where they can't be known.
type Keys = Option<Vec<String>>;

/// The string keys of a dict expression, resolved statically, or None.
fn dict_keys(expr: &Expr, known: &HashMap<String, Keys>) -> Keys {
    match expr {
        Expr::Dict(dict) => dict
            .items
            .iter()
            // A ** spread or a computed key.
            .map(|item| item.key.as_ref().and_then(string_constant).map(str::to_owned))
            .collect(),
        Expr::Call(call) if matches!(&*call.func, Expr::Name(name) if name.id.as_str() == "dict") => {
            let keywords = &call.arguments.keywords;
            if !call.arguments.args.is_empty() || keywords.iter().any(|kw| kw.arg.is_none()) {
                return None;
            }
            Some(
                keywords
                    .iter()
                    .filter_map(|kw| kw.arg.as_ref())
                    .map(|arg| arg.to_string())
                    .collect(),
            )
        }
        _ => variable(expr).and_then(|name| known.get(&name).cloned().flatten()),
    }
}

/// The `known` key for a local (`x`) or an instance attribute (`self.x`).
fn variable(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attr) => match &*attr.value {
            Expr::Name(name) if name.id.as_str() == "self" => Some(format!("self.{}", attr.attr)),
            _ => None,
        },
        _ => None,
    }
}

fn merge(known: &mut HashMap<String, Keys>, name: String, extra: Keys) {
    let current = known.get(&name).cloned().flatten();
    let merged = match (current, extra) {
        (Some(mut current), Some(extra)) => {
            current.extend(extra);
            Some(current)
        }
        _ => None,
    };
    known.insert(name, merged);
}

fn walk_body(body: &[Stmt], known: &mut HashMap<String, Keys>, returns: &mut Vec<Keys>) {
    for stmt in body {
        match stmt {
            Stmt::Assign(assign) if assign.targets.len() == 1 => {
                let target = &assign.targets[0];
                if let Some(name) = variable(target) {
                    let keys = dict_keys(&assign.value, known);
                    known.insert(name, keys);
                } else if let Expr::Subscript(sub) = target
                    && let Some(name) = variable(&sub.value)
                {
                    // x["literal"] = ... adds a known key. Any other key can't be resolved, so the whole dict becomes
                    // unknown rather than silently missing a name.
                    let literal = string_constant(&sub.slice).map(|k| vec![k.to_owned()]);
                    merge(known, name, literal);
                }
            }
            Stmt::AugAssign(aug) => {
                if let Some(name) = variable(&aug.target) {
                    let extra = if aug.op == Operator::BitOr {
                        dict_keys(&aug.value, known)
                    } else {
                        None
                    };
                    merge(known, name, extra);
                }
            }
            Stmt::Expr(expr) => {
                // x.update({...}), x.update(other) and x.update(a=..., b=...).
                let Expr::Call(call) = &*expr.value else {
                    continue;
                };
                let Expr::Attribute(func) = &*call.func else {
                    continue;
                };
                if func.attr.as_str() != "update" {
                    continue;
                }
                let Some(name) = variable(&func.value) else {
                    continue;
                };
                let args = &call.arguments.args;
                let keywords = &call.arguments.keywords;
                let mut extra = match args.len() {
                    0 => Some(Vec::new()),
                    1 => dict_keys(&args[0], known),
                    _ => None,
                };
                if keywords.iter().any(|kw| kw.arg.is_none()) {
                    extra = None; // x.update(**other)
                } else if let Some(extra) = &mut extra {
                    extra.extend(keywords.iter().filter_map(|kw| kw.arg.as_ref()).map(|a| a.to_string()));
                }
                merge(known, name, extra);
            }
            Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    returns.push(dict_keys(value, known));
                }
            }
            // The blocks Python's ast gives as If, For, While, With and Try, not their async or except* forms.
            Stmt::If(stmt) => {
                walk_body(&stmt.body, known, returns);
                for clause in &stmt.elif_else_clauses {
                    walk_body(&clause.body, known, returns);
                }
            }
            Stmt::For(stmt) if !stmt.is_async => {
                walk_body(&stmt.body, known, returns);
                walk_body(&stmt.orelse, known, returns);
            }
            Stmt::While(stmt) => {
                walk_body(&stmt.body, known, returns);
                walk_body(&stmt.orelse, known, returns);
            }
            Stmt::With(stmt) if !stmt.is_async => walk_body(&stmt.body, known, returns),
            Stmt::Try(stmt) if !stmt.is_star => {
                walk_body(&stmt.body, known, returns);
                walk_body(&stmt.orelse, known, returns);
                walk_body(&stmt.finalbody, known, returns);
                for handler in &stmt.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    walk_body(&handler.body, known, returns);
                }
            }
            _ => {}
        }
    }
}

/// The union of every return that resolves, sorted, or None if none does.
fn resolved(returns: Vec<Keys>) -> Keys {
    let resolved: Vec<Vec<String>> = returns.into_iter().flatten().collect();
    if resolved.is_empty() {
        return None;
    }
    let mut keys: Vec<String> = resolved.into_iter().flatten().collect();
    keys.sort();
    keys.dedup();
    Some(keys)
}

/// The names a filter or test file defines: the keys of the dict `FilterModule.filters()` or `TestModule.tests()`
/// returns, or None if they can't be found statically.
///
/// That's `return {...}`, `return dict(...)`, a class attribute returned as `self.x`, a module-level dict returned
/// by name, and locals built from those with `x.update({...})` or `x["k"] = ...`. Returns inside if and try blocks
/// are followed, and the union of every return that resolves is used, so a fallback branch such as
/// `return {f: ... for f in self.filter_map}` doesn't add names.
pub(super) fn filter_test_names(tree: &ModModule, filter: bool) -> Keys {
    let (class_name, method) = if filter {
        ("FilterModule", "filters")
    } else {
        ("TestModule", "tests")
    };
    let mut module_level: HashMap<String, Keys> = HashMap::new();
    for stmt in &tree.body {
        if let Stmt::Assign(assign) = stmt
            && assign.targets.len() == 1
            && let Expr::Name(name) = &assign.targets[0]
        {
            let keys = dict_keys(&assign.value, &module_level);
            module_level.insert(name.id.to_string(), keys);
        }
    }
    for stmt in &tree.body {
        let Stmt::ClassDef(class) = stmt else {
            continue;
        };
        if class.name.as_str() != class_name {
            continue;
        }
        let mut class_attrs: HashMap<String, Keys> = HashMap::new();
        for item in &class.body {
            if let Stmt::Assign(assign) = item
                && assign.targets.len() == 1
                && let Expr::Name(name) = &assign.targets[0]
            {
                class_attrs.insert(name.id.to_string(), dict_keys(&assign.value, &module_level));
            }
        }
        for item in &class.body {
            let Stmt::FunctionDef(func) = item else {
                continue;
            };
            if func.is_async || func.name.as_str() != method {
                continue;
            }
            let mut known: HashMap<String, Keys> = class_attrs
                .iter()
                .map(|(k, v)| (format!("self.{k}"), v.clone()))
                .collect();
            let mut returns = Vec::new();
            walk_body(&func.body, &mut known, &mut returns);
            if let Some(keys) = resolved(returns) {
                return Some(keys);
            }
            // A module-level dict returned by name, such as `return FILTERS`.
            let mut known = module_level.clone();
            let mut returns = Vec::new();
            walk_body(&func.body, &mut known, &mut returns);
            return resolved(returns);
        }
    }
    None
}
