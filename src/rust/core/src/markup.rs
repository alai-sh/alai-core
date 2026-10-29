// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! Ansible's documentation markup, as https://docs.ansible.com/projects/ansible/latest/dev_guide/ansible_markup.html
//! defines it, read as docs.ansible.com renders it, where the page leaves something open.

/// What a part of marked-up text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text with no markup.
    Text,
    /// `B()`.
    Bold,
    /// `I()`.
    Italic,
    /// `C()`.
    Code,
    /// `HORIZONTALLINE`.
    HorizontalLine,
    /// `M()`.
    Module,
    /// `P()`.
    Plugin,
    /// `U()`.
    Url,
    /// `L()`.
    Link,
    /// `R()`, a reference to a heading on docs.ansible.com.
    RstRef,
    /// `E()`.
    EnvVar,
    /// `V()`.
    OptionValue,
    /// `O()`.
    OptionName,
    /// `RV()`.
    ReturnValue,
    /// Markup that isn't well formed.
    Error,
}

impl Kind {
    /// The kind's name, as Python sees it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Bold => "bold",
            Kind::Italic => "italic",
            Kind::Code => "code",
            Kind::HorizontalLine => "horizontal_line",
            Kind::Module => "module",
            Kind::Plugin => "plugin",
            Kind::Url => "url",
            Kind::Link => "link",
            Kind::RstRef => "rst_ref",
            Kind::EnvVar => "env_var",
            Kind::OptionValue => "option_value",
            Kind::OptionName => "option_name",
            Kind::ReturnValue => "return_value",
            Kind::Error => "error",
        }
    }
}

/// A part of marked-up text. Each kind uses only some of the fields, the rest being None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub kind: Kind,
    /// The text shown: for text, `B()`, `I()` and `C()`, and `L()`'s and `R()`'s first parameter. For an error, the
    /// markup as written.
    pub text: Option<String>,
    /// `U()`'s and `L()`'s URL, as written.
    pub url: Option<String>,
    /// `R()`'s anchor.
    pub anchor: Option<String>,
    /// The plugin `M()` or `P()` names, or the plugin `O()` or `RV()` is about, if it isn't the one documented.
    pub fqcn: Option<String>,
    /// `P()`'s plugin type, or `O()`'s or `RV()`'s, with `fqcn`.
    pub plugin_type: Option<String>,
    /// A role's entry point `O()` or `RV()` is about.
    pub entrypoint: Option<String>,
    /// `E()`'s variable, or `O()`'s or `RV()`'s name as written, such as `foo[0].bar`.
    pub name: Option<String>,
    /// `O()`'s or `RV()`'s name as the path to the option, `foo[0].bar` being `foo` and `bar`.
    pub link: Vec<String>,
    /// `V()`'s value, or `O()`'s or `RV()`'s, after `=`.
    pub value: Option<String>,
    /// Whether `O()` or `RV()` is `ignore:`, for one that no longer exists.
    pub ignore: bool,
    /// Why an error isn't well formed.
    pub message: Option<String>,
}

impl Part {
    fn new(kind: Kind) -> Self {
        Part {
            kind,
            text: None,
            url: None,
            anchor: None,
            fqcn: None,
            plugin_type: None,
            entrypoint: None,
            name: None,
            link: Vec::new(),
            value: None,
            ignore: false,
            message: None,
        }
    }

    fn text(kind: Kind, text: &str) -> Self {
        Part {
            text: Some(text.to_owned()),
            ..Part::new(kind)
        }
    }

    fn error(source: &str, message: impl Into<String>) -> Self {
        Part {
            text: Some(source.to_owned()),
            message: Some(message.into()),
            ..Part::new(Kind::Error)
        }
    }
}

/// The macros, by name and kind. The ones taking `\` escapes end at the first `)` not escaped; the others end at the
/// first `)`, so can't hold one.
const MACROS: [(&str, Kind, bool); 13] = [
    ("RV", Kind::ReturnValue, true),
    ("I", Kind::Italic, false),
    ("B", Kind::Bold, false),
    ("M", Kind::Module, false),
    ("U", Kind::Url, false),
    ("L", Kind::Link, false),
    ("R", Kind::RstRef, false),
    ("C", Kind::Code, false),
    ("O", Kind::OptionName, true),
    ("V", Kind::OptionValue, true),
    ("E", Kind::EnvVar, true),
    ("P", Kind::Plugin, true),
    ("HORIZONTALLINE", Kind::HorizontalLine, false),
];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `text` split into its parts. Markup only starts where the character before isn't a letter, digit or `_`, so
/// `aC(x)` is text. Markup that isn't well formed is an error part holding it as written, and an unclosed one ends
/// the text. `role` is whether `text` is in a role's docs, the only place `O(entrypoint:name)` may name another of
/// its entry points.
pub fn parse(text: &str, role: bool) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut plain_start = 0;
    let mut at = 0;
    let last_comma = text.rfind(',');
    while at < text.len() {
        let before = text[..at].chars().next_back();
        let found = if before.is_some_and(is_word) {
            None
        } else {
            MACROS.iter().find(|(name, kind, _)| {
                let rest = &text[at..];
                if *kind == Kind::HorizontalLine {
                    rest.strip_prefix(name)
                        .is_some_and(|after| !after.chars().next().is_some_and(is_word))
                } else {
                    rest.strip_prefix(name).is_some_and(|after| after.starts_with('('))
                }
            })
        };
        let Some(&(name, kind, escaped)) = found else {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        if kind == Kind::HorizontalLine {
            // The spaces and tabs around it go with it.
            let plain = text[plain_start..at].trim_end_matches([' ', '\t']);
            push_text(&mut parts, plain);
            parts.push(Part::new(Kind::HorizontalLine));
            at += name.len();
            at += text[at..].len() - text[at..].trim_start_matches([' ', '\t']).len();
            plain_start = at;
            continue;
        }
        push_text(&mut parts, &text[plain_start..at]);
        let open = at + name.len() + 1;
        let Some((params, end)) = parameters(text, open, kind, escaped, last_comma) else {
            parts.push(Part::error(&text[at..], format!("{name}() isn't closed with )")));
            return parts;
        };
        let source = &text[at..end];
        parts.push(match params.and_then(|params| build(kind, params, role)) {
            Ok(part) => part,
            Err(why) => Part::error(source, why),
        });
        at = end;
        plain_start = at;
    }
    push_text(&mut parts, &text[plain_start..]);
    parts
}

fn push_text(parts: &mut Vec<Part>, text: &str) {
    if !text.is_empty() {
        parts.push(Part::text(Kind::Text, text));
    }
}

/// The parameters of the macro whose `(` ends at `open`, and where it ends, after its `)`; None if it isn't closed.
/// `L()` and `R()` take two, split at the first comma, wherever it is, the spaces around it dropped. One without a
/// comma is an error to its `)`, where docs.ansible.com's parser takes the rest of the text, so the rest is still
/// shown.
#[allow(clippy::type_complexity)]
fn parameters(
    text: &str,
    open: usize,
    kind: Kind,
    escaped: bool,
    last_comma: Option<usize>,
) -> Option<(Result<Vec<String>, String>, usize)> {
    if escaped {
        let mut value = String::new();
        let mut chars = text[open..].char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => value.push(chars.next()?.1),
                ')' => return Some((Ok(vec![value]), open + i + 1)),
                c => value.push(c),
            }
        }
        return None;
    }
    if matches!(kind, Kind::Link | Kind::RstRef) {
        // Knowing the last comma keeps a text of L() without one from searching the rest of it each time.
        let Some(comma) = last_comma
            .filter(|&last| last >= open)
            .and_then(|_| text[open..].find(','))
            .map(|i| open + i)
        else {
            let close = open + text[open..].find(')')?;
            return Some((Err("there's no comma between its two parameters".to_owned()), close + 1));
        };
        let second = comma + 1 + (text[comma + 1..].len() - text[comma + 1..].trim_start_matches(' ').len());
        let close = second + text[second..].find(')')?;
        let first = text[open..comma].trim_end_matches(' ').to_owned();
        return Some((Ok(vec![first, text[second..close].to_owned()]), close + 1));
    }
    let close = open + text[open..].find(')')?;
    Some((Ok(vec![text[open..close].to_owned()]), close + 1))
}

/// A collection's full name as the markup takes one: three or more `.`-separated parts of ASCII letters, digits
/// and `_`, the third on naming a plugin in a subdirectory.
fn is_fqcn(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() >= 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// A plugin type as the markup takes one: ASCII lowercase letters and `_`.
fn is_plugin_type(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// The part a macro's parameters make, or why they don't make one.
fn build(kind: Kind, params: Vec<String>, role: bool) -> Result<Part, String> {
    let mut params = params.into_iter();
    let first = params.next().unwrap_or_default();
    Ok(match kind {
        Kind::Link => Part {
            url: params.next(),
            ..Part::text(kind, &first)
        },
        Kind::RstRef => Part {
            anchor: params.next(),
            ..Part::text(kind, &first)
        },
        Kind::Url => Part {
            url: Some(first),
            ..Part::new(kind)
        },
        Kind::Module if !is_fqcn(&first) => return Err(format!("{first:?} isn't a FQCN")),
        Kind::Module => Part {
            fqcn: Some(first),
            ..Part::new(kind)
        },
        Kind::Plugin => match first.split_once('#') {
            Some((fqcn, plugin_type)) if is_fqcn(fqcn) && is_plugin_type(plugin_type) => Part {
                fqcn: Some(fqcn.to_owned()),
                plugin_type: Some(plugin_type.to_owned()),
                ..Part::new(kind)
            },
            _ => return Err(format!("{first:?} isn't FQCN#type")),
        },
        Kind::EnvVar => Part {
            name: Some(first),
            ..Part::new(kind)
        },
        Kind::OptionValue => Part {
            value: Some(first),
            ..Part::new(kind)
        },
        Kind::OptionName | Kind::ReturnValue => option(kind, &first, role)?,
        _ => Part::text(kind, &first),
    })
}

/// `O()` or `RV()`: `name`, `name=value`, `ignore:name`, `entrypoint:name`, `fqcn#type:name`, or
/// `fqcn#role:entrypoint:name`, each with `=value` allowed. The value is split off first, at the first `=`.
fn option(kind: Kind, param: &str, role: bool) -> Result<Part, String> {
    let invalid = || Err(format!("{param:?} isn't a valid name"));
    let (reference, value) = match param.split_once('=') {
        Some((reference, value)) => (reference, Some(value.to_owned())),
        None => (param, None),
    };
    let mut part = Part {
        value,
        ..Part::new(kind)
    };
    let mut name = reference;
    if let Some(rest) = name.strip_prefix("ignore:") {
        part.ignore = true;
        name = rest;
    } else if let Some((fqcn, rest)) = name.split_once('#') {
        let Some((plugin_type, rest)) = rest.split_once(':') else {
            return invalid();
        };
        if !is_fqcn(fqcn) {
            return invalid();
        }
        if !is_plugin_type(plugin_type) {
            return Err(format!("{plugin_type:?} isn't a plugin type"));
        }
        part.fqcn = Some(fqcn.to_owned());
        part.plugin_type = Some(plugin_type.to_owned());
        name = rest;
        if plugin_type == "role" {
            let Some((entrypoint, rest)) = name.split_once(':') else {
                return Err("a role's option needs its entry point, as role:entrypoint:name".to_owned());
            };
            part.entrypoint = Some(entrypoint.to_owned());
            name = rest;
        }
    } else if let Some((entrypoint, rest)) = name.split_once(':') {
        if !role {
            return Err(format!("{entrypoint:?} is a role's entry point, outside a role's docs"));
        }
        part.entrypoint = Some(entrypoint.to_owned());
        name = rest;
    }
    if name.contains([':', '#']) {
        return invalid();
    }
    part.link = without_brackets(name).split('.').map(str::to_owned).collect();
    part.name = Some(name.to_owned());
    Ok(part)
}

/// `name` with each `[...]` taken out, as it says which list element and not which option.
fn without_brackets(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, Option<String>)> {
        parse(text, false).into_iter().map(|p| (p.kind, p.text)).collect()
    }

    fn one(text: &str) -> Part {
        let mut parts = parse(text, false);
        assert_eq!(parts.len(), 1, "{parts:?}");
        parts.remove(0)
    }

    #[test]
    fn plain_and_formatting() {
        let t = |s: &str| Some(s.to_owned());
        assert_eq!(
            kinds("a C(b) I(c)B(d) e"),
            [
                (Kind::Text, t("a ")),
                (Kind::Code, t("b")),
                (Kind::Text, t(" ")),
                (Kind::Italic, t("c")),
                (Kind::Bold, t("d")),
                (Kind::Text, t(" e")),
            ]
        );
        // The classic markup ends at the first ), so can't hold one.
        assert_eq!(kinds("C(a(b)c)"), [(Kind::Code, t("a(b")), (Kind::Text, t("c)"))]);
        assert_eq!(kinds("C(a\\)b)"), [(Kind::Code, t("a\\")), (Kind::Text, t("b)"))]);
        // Only where the character before isn't part of a word.
        assert_eq!(kinds("aC(x) _C(x) éC(x)"), [(Kind::Text, t("aC(x) _C(x) éC(x)"))]);
        assert_eq!(kinds("x.C(y)")[1], (Kind::Code, t("y")));
        assert_eq!(kinds("X(y)"), [(Kind::Text, t("X(y)"))]);
    }

    #[test]
    fn horizontal_line() {
        let t = |s: &str| Some(s.to_owned());
        assert_eq!(
            kinds("a \t HORIZONTALLINE  b"),
            [(Kind::Text, t("a")), (Kind::HorizontalLine, None), (Kind::Text, t("b"))]
        );
        // Line breaks around it stay; it needs a word boundary on both sides.
        assert_eq!(kinds("a\nHORIZONTALLINE\nb")[0], (Kind::Text, t("a\n")));
        assert_eq!(kinds("xHORIZONTALLINE HORIZONTALLINEx").len(), 1);
    }

    #[test]
    fn links() {
        let link = one("L( text , https://x.y )");
        assert_eq!(
            (link.text.as_deref(), link.url.as_deref()),
            (Some(" text"), Some("https://x.y "))
        );
        let link = one("L(text,https://a,b)");
        assert_eq!(link.url.as_deref(), Some("https://a,b"));
        let rst = one("R(Title,anchor)");
        assert_eq!(
            (rst.text.as_deref(), rst.anchor.as_deref()),
            (Some("Title"), Some("anchor"))
        );
        assert_eq!(one("U( https://x )").url.as_deref(), Some(" https://x "));
        assert_eq!(one("L(text)").kind, Kind::Error);
    }

    #[test]
    fn names() {
        let m = one("M(ns.c.a.b)");
        assert_eq!((m.kind, m.fqcn.as_deref()), (Kind::Module, Some("ns.c.a.b")));
        for bad in ["M(ns.c)", "M(ns..m)", "M(ns.c.m-x)", "M( ns.c.m )", "M(né.c.m)"] {
            assert_eq!(one(bad).kind, Kind::Error, "{bad}");
        }
        let p = one("P(ns.c.p#lookup)");
        assert_eq!(
            (p.fqcn.as_deref(), p.plugin_type.as_deref()),
            (Some("ns.c.p"), Some("lookup"))
        );
        for bad in ["P(ns.c.p)", "P(ns.c.p#Lookup)", "P(ns.c.p#a1)", "P(bad#lookup)"] {
            assert_eq!(one(bad).kind, Kind::Error, "{bad}");
        }
    }

    #[test]
    fn escapes() {
        assert_eq!(one("E(FOO\\)BAR)").name.as_deref(), Some("FOO)BAR"));
        assert_eq!(one("V(a\\\\b\\)c)").value.as_deref(), Some("a\\b)c"));
        assert_eq!(one("V(a\\xb)").value.as_deref(), Some("axb"));
        // An error holds the markup as written.
        assert_eq!(one("O(a\\:b:c)").text.as_deref(), Some("O(a\\:b:c)"));
        // Not closed: an error to the end.
        let parts = parse("a V(b\\) c", false);
        assert_eq!(parts[1].kind, Kind::Error);
        assert_eq!(parts[1].text.as_deref(), Some("V(b\\) c"));
    }

    #[test]
    fn options() {
        let o = one("O(a.b[0].c=x=y)");
        assert_eq!(
            (o.name.as_deref(), o.link.clone(), o.value.as_deref()),
            (
                Some("a.b[0].c"),
                vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
                Some("x=y")
            )
        );
        // The value is split off first, escapes already gone.
        assert_eq!(one("O(a\\=b)").value.as_deref(), Some("b"));
        let o = one("O(ns.c.r#role:ep:name=v)");
        assert_eq!(
            (
                o.fqcn.as_deref(),
                o.plugin_type.as_deref(),
                o.entrypoint.as_deref(),
                o.name.as_deref()
            ),
            (Some("ns.c.r"), Some("role"), Some("ep"), Some("name"))
        );
        let o = one("RV(ns.c.m#module:x.y=1)");
        assert_eq!(
            (o.kind, o.link.len(), o.value.as_deref()),
            (Kind::ReturnValue, 2, Some("1"))
        );
        let o = one("O(ignore:name)");
        assert!(o.ignore && o.name.as_deref() == Some("name"));
        assert_eq!(parse("O(ep:name)", true)[0].entrypoint.as_deref(), Some("ep"));
        assert_eq!(one("O(ep:name)").kind, Kind::Error);
        assert_eq!(one("O(a[x]y.b)").link, ["ay", "b"]);
        assert_eq!(one("O(a[b)").link, ["a[b"]);
        for bad in [
            "O(ns.c.m#role:name)",
            "O(ns.c.m#module:ep:name)",
            "O(bad#module:name)",
            "O(a:b:c)",
            "O(ns.c.m#Bad:x)",
            "O(ignore:a:b)",
        ] {
            assert_eq!(one(bad).kind, Kind::Error, "{bad}");
        }
    }
}
