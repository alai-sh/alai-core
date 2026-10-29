# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import json
from types import MappingProxyType

import pytest

from alai_core.plugins import PluginEntry, Plugins


def line(fqcn: str, type_: str = "module", **fields: object) -> bytes:
    collection, _, name = fqcn.rpartition(".")
    return json.dumps({"v": 1, "type": type_, "name": name, "collection": collection, **fields}).encode()


class TestPluginEntry:
    @pytest.mark.parametrize(
        "text",
        [
            b"not json",
            b"[]",
            b'{"v": 2, "type": "module", "name": "a", "collection": "a.a"}',
            b'{"v": true, "type": "module", "name": "a", "collection": "a.a"}',
            b'{"v": 1, "name": "a", "collection": "a.a"}',
            b'{"v": 1, "type": "module", "name": "a", "collection": "a"}',
            b'{"v": 1.0, "type": "module", "name": "a", "collection": "a.a"}',
            b'{"v": 1, "type": "module", "name": 1, "collection": "a.a"}',
        ],
    )
    def test_skipped(self, text: bytes) -> None:
        assert PluginEntry.parse(text) is None

    def test_older_v_read(self) -> None:
        assert PluginEntry.parse(b'{"v": 0, "type": "module", "name": "a", "collection": "a.a"}') is not None

    def test_fields(self) -> None:
        text = line(
            "a.a.user", since="1.0.0", until="2.0.0", short_description="Users", routing={"redirect": "b.b.user"}
        )

        parsed = PluginEntry.parse(text)

        assert parsed is not None
        assert (parsed.type, parsed.name, parsed.collection, parsed.fqcn) == ("module", "user", "a.a", "a.a.user")
        assert (parsed.since, parsed.until, parsed.short_description) == ("1.0.0", "2.0.0", "Users")
        assert parsed.routing is not None
        assert (parsed.routing.redirect, parsed.routing.raw) == ("b.b.user", {"redirect": "b.b.user"})
        assert parsed.line == text

    def test_fields_not_strings(self) -> None:
        parsed = PluginEntry.parse(line("a.a.x", since=1, short_description=None, routing="x"))

        assert parsed is not None
        assert (parsed.since, parsed.short_description, parsed.routing) == (None, None, None)

    def test_unknown_type_and_fields_kept(self) -> None:
        parsed = PluginEntry.parse(line("a.a.x", "new_type", extra=1))

        assert parsed is not None
        assert parsed.type == "new_type"
        assert parsed.line is not None
        assert json.loads(parsed.line)["extra"] == 1

    def test_writes_compact_json_in_the_spec_field_order(self) -> None:
        routing = {"tombstone": {"removal_version": "2.0.0", "warning_text": 'Use "b" \\ ü'}, "redirect": "b.b.x"}
        entry = PluginEntry("module", "x", "a.a", since="1.0.0", until="2.0.0", short_description="X", routing=routing)

        expected = {
            "v": 1,
            "type": "module",
            "name": "x",
            "collection": "a.a",
            "since": "1.0.0",
            "until": "2.0.0",
            "short_description": "X",
            "routing": {"redirect": "b.b.x", "tombstone": {"removal_version": "2.0.0", "warning_text": 'Use "b" \\ ü'}},
        }
        assert entry.to_json() == json.dumps(expected, separators=(",", ":"), ensure_ascii=False)
        assert PluginEntry.parse(entry.to_json().encode()) == entry

    def test_routing_typed(self) -> None:
        routing = {
            "deprecation": {"removal_date": "2030-01-01", "warning_text": "soon"},
            "tombstone": {"removal_version": 2.0},
            "name": "a mistake for redirect",
        }

        entry = PluginEntry.parse(PluginEntry("module", "x", "a.a", routing=routing).to_json().encode())

        assert entry is not None
        assert entry.routing is not None
        assert entry.routing.redirect is None
        assert entry.routing.deprecation is not None
        assert (entry.routing.deprecation.removal_date, entry.routing.deprecation.warning_text) == (
            "2030-01-01",
            "soon",
        )
        # Not a string, as an older index could have written it, so not given, but kept as written.
        assert entry.routing.tombstone is not None
        assert entry.routing.tombstone.removal_version is None
        assert entry.routing.raw == routing

    def test_routing_only(self) -> None:
        written = PluginEntry("module", "x", "a.a", routing=MappingProxyType({"redirect": "b.b.x"})).to_json()

        assert written == '{"v":1,"type":"module","name":"x","collection":"a.a","routing":{"redirect":"b.b.x"}}'

    @pytest.mark.parametrize(
        ("fields", "match"),
        [
            ({"type": "", "name": "x", "collection": "a.a"}, "must not be empty"),
            ({"type": "module", "name": "", "collection": "a.a"}, "must not be empty"),
            ({"type": "module", "name": "x", "collection": "A.a"}, "collection names are lowercase"),
            ({"type": "module", "name": "x", "collection": "a.a", "routing": {"x": object()}}, "not JSON serializable"),
        ],
    )
    def test_writer_checks(self, fields: dict[str, object], match: str) -> None:
        with pytest.raises((ValueError, TypeError), match=match):
            _ = PluginEntry(**fields)  # pyright: ignore[reportArgumentType]

    def test_read_leniently_written_strictly(self) -> None:
        parsed = PluginEntry.parse(line("Bad.coll.x"))

        assert parsed is not None
        with pytest.raises(ValueError, match=r"invalid collection name 'Bad\.coll'"):
            _ = parsed.to_json()


class TestPlugins:
    def test_every_token_once(self) -> None:
        plugins = Plugins(b"\n".join([line("a.a.user_user"), line("a.a.user"), line("b.b.other")]))

        assert [p.fqcn for p in plugins.search([b"user_user", b"a.a"])] == ["a.a.user_user"]
        assert [p.fqcn for p in plugins.search([b"user"])] == ["a.a.user_user", "a.a.user"]
        assert [p.fqcn for p in plugins.search()] == ["a.a.user_user", "a.a.user", "b.b.other"]

    def test_unreadable_and_blank_lines_skipped(self) -> None:
        plugins = Plugins(b"\n".join([b"", b"not json user", line("a.a.user"), b"   ", line("b.b.user", v=2)]))

        assert [p.fqcn for p in plugins.search([b"user"])] == ["a.a.user"]

    def test_named(self) -> None:
        plugins = Plugins(b"\n".join([line("a.a.copy"), line("b.b.copy_file"), line("c.c.copy", "filter")]))

        assert [(p.type, p.fqcn) for p in plugins.named({"copy"})] == [("module", "a.a.copy"), ("filter", "c.c.copy")]
        assert plugins.named(set()) == []

    def test_names(self) -> None:
        plugins = Plugins(b"\n".join([line("a.a.copy"), line("b.b.user"), b'{"name": "esc\\"aped"}']))

        assert plugins.names() == {"copy", "user"}
