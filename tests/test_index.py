# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import collections.abc as _c
import hashlib
import json
from pathlib import Path

import pytest

from alai_core.index import CollectionEntry, CollectionIndex, Collections, CollectionVersion, Config, NewerFormatError
from alai_core.pep440 import Version as AnsibleVersion
from alai_core.semver import Version

EXAMPLE = Path(__file__).parent / "data" / "index"
CKSUM = "a" * 64
TIME = "2026-10-05T17:30:02Z"
# The spec's field order, which galaxy-sync writes. Each writer's test checks for galaxy-sync's exact bytes, as an index
# file found by SHA-256 gets a new URL if its layout changes.
ORDER = ("v", "name", "vers", "deps", "sha256", "size", "requires_ansible", "yanked", "deprecated", "published")


def line(**fields: object) -> dict[str, object]:
    """A valid index line for ns.coll 1.0.0, with ``fields`` changed (None removes one)."""
    obj: dict[str, object] = {
        "v": 1,
        "name": "ns.coll",
        "vers": "1.0.0",
        "deps": [],
        "sha256": CKSUM,
        "size": 10,
        "yanked": False,
        "published": TIME,
    }
    obj.update(fields)
    return {k: v for k, v in obj.items() if v is not None}


def index_file(*lines: dict[str, object] | str) -> bytes:
    return b"\n".join((json.dumps(x) if isinstance(x, dict) else x).encode() for x in lines)


def config(**changes: object) -> bytes:
    obj: dict[str, object] = {
        "formats": {"1": {"index": "index/{namespace}/{name}.jsonl", "collections": "collections.json"}},
        "dl": "https://dl.example/sha256/{sha256-prefix}/{sha256-checksum}.tar.gz",
        "moved-to": None,
    }
    obj.update(changes)
    return json.dumps(obj).encode()


def new_line(**changes: object) -> CollectionVersion:
    fields: dict[str, object] = {
        "name": "ns.coll",
        "version": Version("1.0.0"),
        "sha256": CKSUM,
        "size": 10,
        "published": TIME,
    }
    fields.update(changes)
    return CollectionVersion(**fields)  # pyright: ignore[reportArgumentType]


class TestConfig:
    def test_example(self) -> None:
        parsed = Config.parse((EXAMPLE / "config.json").read_bytes())

        assert (parsed.format, parsed.index, parsed.collections, parsed.plugins) == (
            1,
            "index/{namespace}/{name}.jsonl",
            "collections.json",
            "plugins.jsonl",
        )
        assert parsed.dl == "https://dl.alai.sh/sha256/{sha256-prefix}/{sha256-checksum}.tar.gz"
        assert parsed.moved_to is None
        assert not parsed.hashed

    @pytest.mark.parametrize("listing", [{}, {"collections": None}, {"collections": ""}])
    def test_no_listing(self, listing: dict[str, object]) -> None:
        parsed = Config.parse(config(formats={"1": {"index": "index/{namespace}/{name}.jsonl", **listing}}))

        assert parsed.collections is None

    def test_highest_supported_format(self) -> None:
        formats = {
            "1": {"index": "v1/{namespace}/{name}.jsonl", "collections": "c1.json"},
            "2": {"index": "v2/{namespace}/{name}.jsonl", "collections": "c2.json"},
        }

        parsed = Config.parse(config(formats=formats))

        assert (parsed.format, parsed.index) == (1, "v1/{namespace}/{name}.jsonl")

    @pytest.mark.parametrize(
        ("template", "hashed"),
        [
            ("index/{namespace}/{name}.jsonl", False),
            ("index/{sha256-prefix}/{sha256-checksum}.jsonl", True),
            ("https://files.example/{namespace}.{name}-{sha256-checksum}.jsonl", True),
        ],
    )
    def test_index_template(self, template: str, hashed: bool) -> None:
        parsed = Config.parse(config(formats={"1": {"index": template, "collections": "c.json"}}))

        assert (parsed.index, parsed.hashed) == (template, hashed)

    def test_moved_to(self) -> None:
        assert Config.parse(config(**{"moved-to": "https://new.example/"})).moved_to == "https://new.example/"

    @pytest.mark.parametrize(("formats", "found"), [({"2": {}, "x": {}}, (2,)), ({}, ())])
    def test_newer_format(self, formats: dict[str, object], found: tuple[int, ...]) -> None:
        with pytest.raises(NewerFormatError) as raised:
            _ = Config.parse(config(formats=formats))

        assert (raised.value.found, raised.value.supported) == (found, 1)

    @pytest.mark.parametrize(
        ("data", "match"),
        [
            (b"[]", "not a JSON object"),
            (b"{", "not valid JSON"),
            (config(formats=[]), "'formats' must be an object"),
            (config(formats={"1": "index/"}), r"'formats\.1' must be an object"),
            (config(formats={"1": {"collections": "c.json"}}), r"'formats\.1' must have an 'index' template"),
            (
                config(formats={"1": {"index": "i/{namespace}/{name}", "collections": 1}}),
                r"'formats\.1\.collections' must be a path",
            ),
            (
                config(formats={"1": {"index": "index/{sha256-prefix}/{sha256-checksum}.jsonl"}}),
                r"'formats\.1\.index' finds files by SHA-256, which only a 'collections' listing gives",
            ),
            (
                config(formats={"1": {"index": "index/{name}.jsonl", "collections": "c.json"}}),
                r"'formats\.1\.index' must be a URL template naming each file",
            ),
            (
                config(formats={"1": {"index": "index/{sha256-prefix}.jsonl", "collections": "c.json"}}),
                r"'formats\.1\.index' must be a URL template naming each file",
            ),
            (config(dl="https://dl.example/artifact.tar.gz"), "'dl' must be a URL template with a placeholder"),
            (config(dl=None), "'dl' must be a URL template"),
            (config(**{"moved-to": 1}), "'moved-to' must be a URL or null"),
            (
                config(formats={"1": {"index": "i/{namespace}/{name}", "collections": "c.json", "plugins": 1}}),
                r"'formats\.1\.plugins' must be a path",
            ),
        ],
    )
    def test_invalid(self, data: bytes, match: str) -> None:
        with pytest.raises(ValueError, match=match):
            _ = Config.parse(data)

    @pytest.mark.parametrize(
        ("template", "sha256", "expected"),
        [
            ("index/{namespace}/{name}.jsonl", None, "index/ns/coll.jsonl"),
            ("index/{sha256-prefix}/{sha256-checksum}.jsonl", "ab" + "c" * 62, f"index/ab/ab{'c' * 62}.jsonl"),
        ],
    )
    def test_index_path(self, template: str, sha256: str | None, expected: str) -> None:
        parsed = Config(index=template, collections="c.json", dl="{sha256-checksum}")

        assert parsed.index_path(name="ns.coll", sha256=sha256) == expected

    def test_index_path_needs_sha256(self) -> None:
        parsed = Config(index="i/{sha256-checksum}", collections="c.json", dl="{sha256-checksum}")

        with pytest.raises(ValueError, match=r"ns\.coll has no sha256"):
            _ = parsed.index_path(name="ns.coll")

    def test_artifact_path(self) -> None:
        dl = "https://dl.example/{namespace}/{name}/{version}/{sha256-prefix}/{sha256-checksum}.tar.gz"
        parsed = Config(index="i/{namespace}/{name}", dl=dl)

        assert (
            parsed.artifact_path(name="ns.coll", version=Version("1.0.0+build.1"), sha256=CKSUM)
            == f"https://dl.example/ns/coll/1.0.0%2Bbuild.1/aa/{CKSUM}.tar.gz"
        )

    def test_writes_json_indented_by_two(self) -> None:
        written = Config(
            index="index/{sha256-prefix}/{sha256-checksum}.jsonl",
            collections="collections.json",
            plugins="plugins.jsonl",
            dl="https://dl.alai.sh/sha256/{sha256-prefix}/{sha256-checksum}.tar.gz",
        ).to_json()

        expected = {
            "formats": {
                "1": {
                    "index": "index/{sha256-prefix}/{sha256-checksum}.jsonl",
                    "collections": "collections.json",
                    "plugins": "plugins.jsonl",
                }
            },
            "dl": "https://dl.alai.sh/sha256/{sha256-prefix}/{sha256-checksum}.tar.gz",
            "moved-to": None,
        }
        assert written == json.dumps(expected, indent=2) + "\n"
        assert Config.parse(written.encode()).index == "index/{sha256-prefix}/{sha256-checksum}.jsonl"

    def test_writer_checks(self) -> None:
        with pytest.raises(ValueError, match="finds files by SHA-256"):
            _ = Config(index="i/{sha256-checksum}", dl="{sha256-checksum}")


class TestCollections:
    def test_example(self) -> None:
        parsed = Collections.parse((EXAMPLE / "collections.json").read_bytes())

        assert list(parsed) == ["alai_example.demo", "community.general", "community.library_inventory_filtering_v1"]
        demo = parsed.get("alai_example.demo")
        assert demo is not None
        assert demo.updated == "2026-09-20T09:00:00Z"
        assert (
            demo.sha256 == hashlib.sha256((EXAMPLE / "index" / "alai_example" / "demo.jsonl").read_bytes()).hexdigest()
        )
        assert "community.general" in parsed
        assert len(parsed) == 3

    @pytest.mark.parametrize("value", ["ABC", "a" * 63, "A" * 64, 1, None])
    def test_malformed_sha256_ignored(self, value: object) -> None:
        data = json.dumps({"v": 1, "namespaces": {"ns": {"coll": {"updated": TIME, "sha256": value}}}}).encode()

        assert Collections.parse(data) == Collections({"ns.coll": CollectionEntry(TIME)})

    def test_missing_fields(self) -> None:
        parsed = Collections.parse(b'{"v": 1, "namespaces": {"ns": {"coll": {}}, "x": {"y": 1}}}')

        assert [(name, entry.updated) for name in parsed if (entry := parsed.get(name))] == [
            ("ns.coll", ""),
            ("x.y", ""),
        ]

    def test_newer_version(self) -> None:
        with pytest.raises(NewerFormatError) as raised:
            _ = Collections.parse(b'{"v": 2, "namespaces": {}}')

        assert (raised.value.found, raised.value.supported) == ((2,), 1)

    @pytest.mark.parametrize(
        ("data", "match"),
        [
            (b'{"namespaces": {}}', "expected 'v': 1 and a 'namespaces' object"),
            (b'{"v": 1, "namespaces": []}', "expected 'v': 1"),
            (b'{"v": 1, "namespaces": {"ns": []}}', "namespace 'ns' must be an object"),
        ],
    )
    def test_invalid(self, data: bytes, match: str) -> None:
        with pytest.raises(ValueError, match=match):
            _ = Collections.parse(data)

    def test_writes_json_indented_by_one_and_sorted(self) -> None:
        listing = Collections(
            {
                "ns.b": CollectionEntry(TIME, "b" * 64),
                "ns.a": CollectionEntry(TIME),
                "other.x": CollectionEntry(TIME, "c" * 64),
            }
        )

        expected = {
            "v": 1,
            "namespaces": {
                "ns": {"a": {"updated": TIME}, "b": {"updated": TIME, "sha256": "b" * 64}},
                "other": {"x": {"updated": TIME, "sha256": "c" * 64}},
            },
        }
        assert listing.to_json() == json.dumps(expected, indent=1) + "\n"
        assert Collections.parse(listing.to_json().encode()) == listing
        assert Collections({}).to_json() == json.dumps({"v": 1, "namespaces": {}}, indent=1) + "\n"

    @pytest.mark.parametrize(
        ("make", "match"),
        [
            (lambda: CollectionEntry("2026-10-05"), "'updated' must be an RFC 3339 time"),
            (lambda: CollectionEntry(TIME, "ABC"), "'sha256' must be a lowercase hex sha256"),
            (lambda: Collections({"Not.Valid": CollectionEntry(TIME)}), "invalid collection name 'Not.Valid'"),
        ],
    )
    def test_writer_checks(self, make: _c.Callable[[], object], match: str) -> None:
        with pytest.raises(ValueError, match=match):
            _ = make()

    def test_read_leniently_written_strictly(self) -> None:
        parsed = Collections.parse(b'{"v": 1, "namespaces": {"ns": {"coll": {}}}}')

        with pytest.raises(ValueError, match=r"ns\.coll: 'updated' must be an RFC 3339 time"):
            _ = parsed.to_json()
        with pytest.raises(ValueError, match=r"invalid collection name 'Bad-NS\.x'"):
            _ = Collections.parse(
                b'{"v": 1, "namespaces": {"Bad-NS": {"x": {"updated": "2026-10-05T17:30:02Z"}}}}'
            ).to_json()


class TestCollectionIndex:
    def test_example(self) -> None:
        parsed = CollectionIndex.parse(
            (EXAMPLE / "index" / "alai_example" / "demo.jsonl").read_bytes(), "alai_example.demo"
        )

        assert [v.version.text for v in parsed.lines] == ["0.1.0", "0.1.1", "0.2.0-rc.1", "0.2.0"]
        assert [v.yanked for v in parsed.lines] == [False, True, False, False]
        newest = parsed.lines[-1]
        assert [(name, str(req)) for name, req in newest.deps] == [("community.general", ">=13.0.0,<14.0.0")]
        assert newest.deps[0][1].contains(Version("13.5.0"))
        assert newest.requires_ansible is not None
        assert newest.requires_ansible.contains(AnsibleVersion("2.18.0"))
        assert newest.problem is None
        assert (parsed.newer, parsed.invalid) == (0, ())

    def test_example_round_trip(self) -> None:
        for path in (EXAMPLE / "index").rglob("*.jsonl"):
            name = ".".join(path.relative_to(EXAMPLE / "index").with_suffix("").parts)
            data = path.read_text()

            assert CollectionIndex.parse(data.encode(), name).to_jsonl() == data

    def test_optional_fields(self) -> None:
        parsed = CollectionIndex.parse(index_file(line(deprecated=True, requires_ansible=None, sig=[])), "ns.coll")

        [version] = parsed.lines
        assert version.deprecated is True
        assert version.requires_ansible is None

    def test_blank_lines_and_unknown_fields(self) -> None:
        parsed = CollectionIndex.parse(index_file("", line(future_field={"x": 1}), "   ", ""), "ns.coll")

        assert len(parsed.lines) == 1
        assert parsed.invalid == ()

    def test_newer_lines_skipped_and_counted(self) -> None:
        parsed = CollectionIndex.parse(
            index_file(line(), line(v=2, vers="2.0.0", deps="a new shape"), line(v=3)), "ns.coll"
        )

        assert [v.version.text for v in parsed.lines] == ["1.0.0"]
        assert parsed.newer == 2

    @pytest.mark.parametrize(
        ("bad", "match"),
        [
            ("{", "not valid JSON"),
            ("[1]", "not a JSON object"),
            (line(v=0), "'v' must be 1"),
            (line(v=True), "'v' must be 1"),
            # Python's reader took 1.0 as 1. Only galaxy-sync writes lines, and it writes 1.
            (line(v=1.0), "'v' must be 1"),
            (line(name="ns.other"), "'name' must be 'ns.coll', the file's collection"),
            (line(vers="1.0"), "invalid version '1.0'"),
            (line(vers=None), "'vers' is required and must be a string"),
            (line(sha256="ABC"), "'sha256' must be a lowercase hex sha256"),
            (line(size=-1), "'size' must not be negative"),
            (line(size=True), "'size' is required and must be an integer"),
            (line(yanked="no"), "'yanked' is required and must be a boolean"),
            (line(deps={}), "'deps' is required and must be an array"),
            (line(deps=[{"name": "a.b"}]), "each of 'deps' must have a 'name' and a 'req' string"),
            (line(deprecated=1), "'deprecated' must be true or a message"),
            (line(requires_ansible=2), "'requires_ansible' must be a string"),
        ],
    )
    def test_invalid_line_skipped(self, bad: dict[str, object] | str, match: str) -> None:
        parsed = CollectionIndex.parse(index_file(line(), bad), "ns.coll")

        assert [v.version.text for v in parsed.lines] == ["1.0.0"]
        [(number, why)] = parsed.invalid
        assert number == 2
        assert match in why

    def test_version_checked_first(self) -> None:
        parsed = CollectionIndex.parse(index_file(line(vers="1.0", sha256="ABC", size=-1)), "ns.coll")

        assert parsed.invalid[0][1].startswith("invalid version '1.0'")

    def test_byte_order_mark_and_crlf(self) -> None:
        data = b" \t\r\n\xef\xbb\xbf" + json.dumps(line()).encode() + b"\r\n"

        parsed = CollectionIndex.parse(data, "ns.coll")

        assert [v.version.text for v in parsed.lines] == ["1.0.0"]
        assert parsed.invalid == ()

    def test_null_optional_fields(self) -> None:
        parsed = CollectionIndex.parse(
            index_file(
                json.dumps(line()).replace("{", '{"requires_ansible": null, ', 1),
                line(vers="2.0.0") | {"deprecated": None},
            ),
            "ns.coll",
        )

        assert [v.version.text for v in parsed.lines] == ["1.0.0"]
        assert parsed.lines[0].requires_ansible is None
        assert parsed.invalid == ((2, "2.0.0: 'deprecated' must be true or a message"),)

    def test_duplicate_version(self) -> None:
        parsed = CollectionIndex.parse(index_file(line(), line(sha256="b" * 64)), "ns.coll")

        assert [v.sha256 for v in parsed.lines] == [CKSUM]
        assert parsed.invalid == ((2, "version 1.0.0 is listed more than once"),)

    @pytest.mark.parametrize(
        ("fields", "problem"),
        [
            ({"deps": [{"name": "a.b", "req": "^3.0.0"}]}, "dependency a.b: invalid version requirement '^3.0.0'"),
            ({"deps": [{"name": "Not.Valid", "req": "*"}]}, "dependency Not.Valid: invalid collection name"),
            ({"requires_ansible": "2.17"}, "requires_ansible '2.17': invalid specifier '2.17'"),
        ],
    )
    def test_unusable_version_kept(self, fields: dict[str, object], problem: str) -> None:
        parsed = CollectionIndex.parse(index_file(line(**fields)), "ns.coll")

        assert parsed.invalid == ()
        [version] = parsed.lines
        assert version.problem is not None
        assert version.problem.startswith(problem)

    def test_unusable_raises_rather_than_hides(self) -> None:
        deps = [{"name": "a.b", "req": "^1"}, {"name": "c.d", "req": ">=1"}]

        [version] = CollectionIndex.parse(index_file(line(deps=deps, requires_ansible="2.17")), "ns.coll").lines

        assert version.problem is not None
        assert version.problem.startswith("dependency a.b:")
        with pytest.raises(ValueError, match=r"ns\.coll 1\.0\.0: dependency a\.b: invalid version requirement"):
            _ = version.deps
        with pytest.raises(ValueError, match=r"ns\.coll 1\.0\.0: requires_ansible '2\.17'"):
            _ = version.requires_ansible

    def test_deprecated_message_read_as_deprecated(self) -> None:
        [version] = CollectionIndex.parse(index_file(line(deprecated="use other.coll")), "ns.coll").lines

        assert version.deprecated is True
        assert '"deprecated":true' in version.to_json()


class TestCollectionVersion:
    def test_writes_compact_json_in_the_spec_field_order(self) -> None:
        written = new_line(
            deps=[("other.coll", ">=1.0.0")],
            requires_ansible=">=2.17",
            deprecated=True,
            yanked=True,
        ).to_json()

        expected = line(
            deps=[{"name": "other.coll", "req": ">=1.0.0"}],
            requires_ansible=">=2.17",
            yanked=True,
            deprecated=True,
        )
        assert written == json.dumps({k: expected[k] for k in ORDER}, separators=(",", ":"), ensure_ascii=False)

    def test_escapes_as_python_does(self) -> None:
        text = 'use "other" \\ \n\t\x01 ünïcødé 🙂'
        expected = line(deps=[{"name": "a.b", "req": text}])

        written = new_line(deps=[("a.b", text)]).to_json(allow_unreadable=True)

        assert written == json.dumps(
            {k: expected[k] for k in ORDER if k in expected}, separators=(",", ":"), ensure_ascii=False
        )

    def test_unreadable_refused_unless_allowed(self) -> None:
        unreadable = new_line(deps=[("a.b", "^3.0.0")], requires_ansible="2.17")

        assert unreadable.problem is not None
        with pytest.raises(ValueError, match=r"ns\.coll 1\.0\.0: dependency a\.b"):
            _ = unreadable.to_json()
        written = json.loads(unreadable.to_json(allow_unreadable=True))
        assert (written["deps"], written["requires_ansible"]) == ([{"name": "a.b", "req": "^3.0.0"}], "2.17")

    @pytest.mark.parametrize(
        ("changes", "match"),
        [
            ({"name": "NS.coll"}, "collection names are lowercase"),
            ({"sha256": "ABC"}, "'sha256' must be a lowercase hex sha256"),
            ({"published": "2026-10-05 17:30:02"}, "'published' must be an RFC 3339 time"),
            ({"published": "2026-13-05T17:30:02Z"}, "'published' must be an RFC 3339 time"),
            ({"published": "2026-02-29T17:30:02Z"}, "'published' must be an RFC 3339 time"),
        ],
    )
    def test_writer_checks(self, changes: dict[str, object], match: str) -> None:
        with pytest.raises(ValueError, match=match):
            _ = new_line(**changes)

    def test_read_leniently_written_strictly(self) -> None:
        [read] = CollectionIndex.parse(index_file(line(published="yesterday")), "ns.coll").lines
        [bad_name] = CollectionIndex.parse(index_file(line(name="Bad.Coll")), "Bad.Coll").lines

        with pytest.raises(ValueError, match="'published' must be an RFC 3339 time"):
            _ = read.to_json()
        with pytest.raises(ValueError, match=r"invalid collection name 'Bad\.Coll'"):
            _ = bad_name.to_json()

    def test_leap_day(self) -> None:
        assert new_line(published="2028-02-29T17:30:02Z").published == "2028-02-29T17:30:02Z"


class TestWritingCollectionIndex:
    def test_round_trip(self) -> None:
        lines = [new_line(), new_line(version=Version("1.1.0"), deps=[("other.coll", ">=1")])]

        written = CollectionIndex("ns.coll", lines).to_jsonl()

        assert written.count("\n") == 2
        assert CollectionIndex.parse(written.encode(), "ns.coll").lines == tuple(lines)

    @pytest.mark.parametrize(
        ("lines", "match"),
        [
            ([new_line(), new_line()], "version 1.0.0 is listed more than once"),
            ([new_line(name="ns.other")], "doesn't belong in ns.coll's index file"),
        ],
    )
    def test_writer_checks(self, lines: list[CollectionVersion], match: str) -> None:
        with pytest.raises(ValueError, match=match):
            _ = CollectionIndex("ns.coll", lines)
