# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import pytest

from alai_core.markup import Part, parse


def shown(text: str) -> list[tuple[str, str | None]]:
    return [(p.kind, p.text) for p in parse(text)]


def one(text: str) -> Part:
    parts = parse(text)
    assert len(parts) == 1, parts
    return parts[0]


def test_formatting() -> None:
    assert shown("a C(b) I(c)B(d) HORIZONTALLINE e") == [
        ("text", "a "),
        ("code", "b"),
        ("text", " "),
        ("italic", "c"),
        ("bold", "d"),
        ("horizontal_line", None),
        ("text", "e"),
    ]


def test_only_at_a_word_boundary() -> None:
    assert shown("aC(x) _C(x) éC(x)") == [("text", "aC(x) _C(x) éC(x)")]
    assert shown("(C(x))") == [("text", "("), ("code", "x"), ("text", ")")]


def test_classic_markup_ends_at_the_first_parenthesis() -> None:
    assert shown("C(a(b)c)") == [("code", "a(b"), ("text", "c)")]
    # A URL holding ) needs it as %29.
    url = parse("U(https://x/Foo_(bar))")
    assert [(p.kind, p.url or p.text) for p in url] == [("url", "https://x/Foo_(bar"), ("text", ")")]


@pytest.mark.parametrize(
    ("text", "fields"),
    [
        ("M(ns.coll.sub.mod)", {"kind": "module", "fqcn": "ns.coll.sub.mod"}),
        ("P(ns.coll.p#lookup)", {"kind": "plugin", "fqcn": "ns.coll.p", "plugin_type": "lookup"}),
        ("U(https://x)", {"kind": "url", "url": "https://x"}),
        ("L(Text , https://x)", {"kind": "link", "text": "Text", "url": "https://x"}),
        ("R(Title,anchor)", {"kind": "rst_ref", "text": "Title", "anchor": "anchor"}),
        ("E(FOO\\)BAR)", {"kind": "env_var", "name": "FOO)BAR"}),
        ("V(a\\,b)", {"kind": "option_value", "value": "a,b"}),
    ],
)
def test_references(text: str, fields: dict[str, str]) -> None:
    part = one(text)
    assert {k: getattr(part, k) for k in fields} == fields


def test_option_names() -> None:
    o = one("O(ns.coll.r#role:main:a[0].b=x=y)")
    assert (o.kind, o.fqcn, o.plugin_type, o.entrypoint, o.name, o.link, o.value, o.ignore) == (
        "option_name",
        "ns.coll.r",
        "role",
        "main",
        "a[0].b",
        ("a", "b"),
        "x=y",
        False,
    )
    rv = one("RV(ignore:gone)")
    assert (rv.kind, rv.name, rv.ignore) == ("return_value", "gone", True)
    assert parse("O(other:opt)", role=True)[0].entrypoint == "other"
    assert parse("RV(other:opt)", role=True)[0].entrypoint == "other"


@pytest.mark.parametrize(
    "text",
    ["M(ns.coll)", "P(ns.coll.p)", "O(ns.c.m#role:name)", "O(a:b:c)", "O(other:opt)", "L(no comma)", "O(ns.c.m#Bad:x)"],
)
def test_errors_keep_the_markup(text: str) -> None:
    part = one(text)
    assert (part.kind, part.text) == ("error", text)
    assert part.message


def test_unclosed_takes_the_rest() -> None:
    assert [(p.kind, p.text) for p in parse("a V(b\\) c")] == [("text", "a "), ("error", "V(b\\) c")]


def test_a_missing_comma_keeps_the_rest() -> None:
    assert shown("See L(https://x) for more.") == [
        ("text", "See "),
        ("error", "L(https://x)"),
        ("text", " for more."),
    ]


def test_unused_fields() -> None:
    part = one("C(x)")
    assert (part.url, part.fqcn, part.name, part.link, part.value, part.ignore, part.message) == (
        None,
        None,
        None,
        (),
        None,
        False,
        None,
    )
    assert part == one("C(x)")
    assert repr(part) == 'Part(kind="code", text="x")'
