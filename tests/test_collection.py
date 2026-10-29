# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import json

import pytest

from alai_core.collection import EdaRuntime, Manifest, Metadata, MetadataError, Runtime, check_name
from alai_core.pep440 import SpecifierSet
from alai_core.semver import Version
from alai_core.yaml import YamlError, YamlWarning

OK = "namespace: ns\nname: coll\nversion: 1.0.0\nreadme: README.md\nauthors: [me]\n"


def fields(e: pytest.ExceptionInfo[MetadataError]) -> list[str | None]:
    return [field for field, _ in e.value.errors]


class TestMetadata:
    def test_fields(self) -> None:
        m = Metadata.from_galaxy_yml(f"{OK}dependencies: {{a.b: '>=1', c.d: '*'}}\n".encode())

        assert [m.get(k) for k in ("namespace", "name", "version")] == ["ns", "coll", "1.0.0"]
        assert m.get("dependencies") == {"a.b": ">=1", "c.d": "*"}
        assert (m.get_fqcn(), m.get_version()) == ("ns.coll", Version("1.0.0"))
        assert [(n, r.text) for n, r in m.get_dependencies().items()] == [("a.b", ">=1"), ("c.d", "*")]

    def test_as_written(self) -> None:
        # Read without checking, so a caller that doesn't need a value isn't stopped by it.
        m = Metadata.from_galaxy_yml(b"name: [x]\nversion: 1.0\ndependencies: {a.b: '^1'}\n")

        assert [m.get(k) for k in ("namespace", "name", "version", "dependencies")] == [None, ["x"], 1.0, {"a.b": "^1"}]

    def test_no_dependencies(self) -> None:
        assert Metadata.from_galaxy_yml(b"").get_dependencies() == {}

    def test_every_dependency_problem(self) -> None:
        text = "namespace: ns\nname: coll\ndependencies: {Bad.x: '*', a.b: '^1', ns.coll: '*', c.d: 1, e.f: '>=1'}\n"

        with pytest.raises(MetadataError) as e:
            _ = Metadata.from_galaxy_yml(text.encode()).get_dependencies()

        assert fields(e) == [
            "dependencies.Bad.x",
            "dependencies.a.b",
            "dependencies.ns.coll",
            "dependencies.c.d",
        ]
        assert dict(e.value.errors)["dependencies.ns.coll"] == "the collection can't depend on itself"

    @pytest.mark.parametrize(
        ("text", "message"),
        [
            ("name: coll\n", "'namespace' and 'name' are required strings"),
            ("namespace: Ns\nname: coll\n", "collection names are lowercase"),
        ],
    )
    def test_fqcn_refused(self, text: str, message: str) -> None:
        with pytest.raises(MetadataError, match=message) as e:
            _ = Metadata.from_galaxy_yml(text.encode()).get_fqcn()

        assert fields(e) == ["name"]

    @pytest.mark.parametrize(
        ("text", "message"),
        [
            ("", "'version' is required and must be a string"),
            ("version: 1.10\n", "invalid version '1.10'"),
            ("version: 1.0\n", "'version' is required and must be a string"),
            ("version: '1.0'\n", "'1.0'"),
        ],
    )
    def test_version_refused(self, text: str, message: str) -> None:
        with pytest.raises(MetadataError, match=message) as e:
            _ = Metadata.from_galaxy_yml(text.encode()).get_version()

        assert fields(e) == ["version"]

    def test_dependencies_not_a_mapping(self) -> None:
        with pytest.raises(MetadataError, match="must be a mapping") as e:
            _ = Metadata.from_galaxy_yml(b"dependencies: [a.b]\n").get_dependencies()

        assert fields(e) == ["dependencies"]

    def test_named(self) -> None:
        m = Metadata.from_galaxy_yml(b"namespace: ns\n", name="galaxy.yml")

        with pytest.raises(MetadataError) as e:
            _ = m.get_version()

        assert str(e.value) == "galaxy.yml: version: 'version' is required and must be a string, such as '1.0.0'"

    def test_not_a_mapping(self) -> None:
        with pytest.raises(MetadataError, match="expected a mapping at the top level") as e:
            _ = Metadata.from_galaxy_yml(b"- a\n")

        assert fields(e) == [None]


class TestRawKeys:
    def test_get(self) -> None:
        m = Metadata.from_galaxy_yml(f"{OK}build_ignore: ['*.tar.gz']\nmanifest: null\n".encode())

        assert m.get("build_ignore") == ["*.tar.gz"]
        assert m.get("manifest", "unset") is None
        assert m.get("nope", "unset") == "unset"
        assert ("manifest" in m, "nope" in m) == (True, False)


SHA256 = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"


def manifest(info: object = None, **top: object) -> bytes:
    data: dict[str, object] = {
        "collection_info": {"namespace": "ns", "name": "c", "version": "1.0.0"} if info is None else info,
        "file_manifest_file": {
            "name": "FILES.json",
            "ftype": "file",
            "chksum_type": "sha256",
            "chksum_sha256": SHA256,
            "format": 1,
        },
        "format": 1,
    }
    data.update(top)
    return json.dumps(data).encode()


class TestManifest:
    def test_parse(self) -> None:
        parsed = Manifest.parse(manifest({"namespace": "ns", "name": "c", "version": "1.0.0", "foo": 1}))

        m = parsed.collection_info
        assert [m.get(k) for k in ("namespace", "name", "version", "foo")] == ["ns", "c", "1.0.0", 1]
        assert (m.get_fqcn(), m.get_version()) == ("ns.c", Version("1.0.0"))
        assert parsed.files_sha256 == SHA256

    def test_collection_info_checked_when_asked(self) -> None:
        m = Manifest.parse(manifest({})).collection_info

        assert "version" not in m
        with pytest.raises(MetadataError):
            _ = m.get_fqcn()

    @pytest.mark.parametrize(
        ("top", "field"),
        [
            ({"collection_info": []}, "collection_info"),
            ({"format": 2}, "format"),
            ({"file_manifest_file": None}, "file_manifest_file"),
            ({"file_manifest_file": {"name": "FILES.json"}}, "file_manifest_file.ftype"),
        ],
    )
    def test_refused(self, top: dict[str, object], field: str) -> None:
        with pytest.raises(MetadataError) as e:
            _ = Manifest.parse(manifest(**top))

        assert field in fields(e)

    def test_every_problem(self) -> None:
        with pytest.raises(MetadataError) as e:
            _ = Manifest.parse(b"{}", name="MANIFEST.json")

        assert fields(e) == ["collection_info", "format", "file_manifest_file"]
        assert all(line.startswith("MANIFEST.json: ") for line in str(e.value).splitlines())

    def test_not_json(self) -> None:
        with pytest.raises(ValueError, match=r"^MANIFEST\.json: not valid JSON: "):
            _ = Manifest.parse(b"{", name="MANIFEST.json")

    def test_named(self) -> None:
        with pytest.raises(MetadataError, match=r"^MANIFEST\.json: version: "):
            _ = Manifest.parse(manifest({}), name="MANIFEST.json").collection_info.get_version()


class TestYaml:
    def test_error(self) -> None:
        with pytest.raises(YamlError, match=r"^galaxy\.yml: "):
            _ = Metadata.from_galaxy_yml(b"a: [", name="galaxy.yml")

    def test_warnings(self) -> None:
        with pytest.warns(YamlWarning, match=r"^galaxy\.yml: found duplicate mapping key 'a'"):
            _ = Metadata.from_galaxy_yml(f"{OK}a: 1\na: 2\n".encode(), name="galaxy.yml")

    def test_warnings_ignored(self) -> None:
        got: list[YamlWarning] = []

        _ = Metadata.from_galaxy_yml(f"{OK}a: 1\na: 2\n".encode(), on_warning=None)
        _ = Runtime.parse(b"a: 1\na: 2\n", on_warning=got.append)

        assert [w.kind for w in got] == ["duplicate_key"]


class TestRuntime:
    def test_fields(self) -> None:
        text = (
            "requires_ansible: '>=2.17'\n"
            "plugin_routing:\n"
            "  modules:\n"
            "    old: &e {redirect: ns.c.new, action_plugin: ns.c.act, extra: yes}\n"
            "    older: *e\n"
            "    1: {tombstone: {removal_version: 2.0, warning_text: gone}}\n"
            "    soon: {deprecation: {removal_date: 2030-01-01}}\n"
            "    bad: x\n"
            "  filter: nope\n"
        )

        runtime = Runtime.parse(text.encode())

        assert runtime.requires_ansible == ">=2.17"
        modules = runtime.plugin_routing["modules"]
        assert list(modules) == ["old", "older", "1", "soon"]
        assert (modules["old"].redirect, modules["old"].action_plugin) == ("ns.c.new", "ns.c.act")
        assert modules["old"].raw is modules["older"].raw
        tombstone = modules["1"].tombstone
        assert tombstone is not None
        assert (tombstone.removal_version, tombstone.removal_date, tombstone.warning_text) == ("2.0", None, "gone")
        deprecation = modules["soon"].deprecation
        assert deprecation is not None
        assert deprecation.removal_date == "2030-01-01"
        assert "filter" not in runtime.plugin_routing

    def test_every_scalar_a_string(self) -> None:
        # As ansible-core reads the file, with PyYAML's BaseLoader.
        entry = Runtime.parse(b"plugin_routing: {modules: {a: {x: yes, y: ~, z: 2.10, w: 1, v: {<<: {q: 1}}}}}\n")

        assert entry.plugin_routing["modules"]["a"].raw == {
            "x": "yes",
            "y": "~",
            "z": "2.10",
            "w": "1",
            "v": {"<<": {"q": "1"}},
        }

    @pytest.mark.parametrize(
        ("text", "raw", "message"),
        [
            ("requires_ansible: 2.15\n", "2.15", "expected PEP 440 such as '>=2.17'"),
            ("requires_ansible: '2.15+'\n", "2.15+", "expected PEP 440 such as '>=2.17'"),
            ("requires_ansible: [a]\n", ["a"], "^requires_ansible: 'requires_ansible' must be a string$"),
        ],
    )
    def test_requires_ansible_checked(self, text: str, raw: object, message: str) -> None:
        runtime = Runtime.parse(text.encode())

        assert runtime.requires_ansible == raw
        with pytest.raises(MetadataError, match=message) as e:
            _ = runtime.get_requires_ansible()

        assert fields(e) == ["requires_ansible"]

    def test_requires_ansible(self) -> None:
        assert Runtime.parse(b"requires_ansible: '>=2.17'\n").get_requires_ansible() == SpecifierSet(">=2.17")

    @pytest.mark.parametrize("text", ["", "''\n"])
    def test_empty(self, text: str) -> None:
        runtime = Runtime.parse(text.encode())

        assert (runtime.requires_ansible, runtime.get_requires_ansible(), runtime.plugin_routing) == (None, None, {})

    @pytest.mark.parametrize("text", ["- a\n", "null\n", "~\n", "text\n"])
    def test_not_a_mapping(self, text: str) -> None:
        # ansible-core can't load it, but it's read as empty. null and ~ are strings to ansible-core.
        assert Runtime.parse(text.encode()).plugin_routing == {}


class TestEdaRuntime:
    def test_fields(self) -> None:
        data = (
            b"plugin_routing:\n"
            b"  event_source:\n"
            b"    old: {redirect: ns.c.new, deprecation: {removal_version: 3.0, warning_text: gone}}\n"
            b"  event_filter:\n"
            b"    noop: {tombstone: {warning_text: bye}, deprecation: null}\n"
            b"  modules:\n"
            b"    x: {redirect: a.b.c}\n"
        )

        routing = EdaRuntime.parse(data).plugin_routing

        # Every type is kept, though ansible-rulebook reads only event sources and filters.
        assert list(routing) == ["event_source", "event_filter", "modules"]
        assert routing["modules"]["x"].redirect == "a.b.c"
        old = routing["event_source"]["old"]
        assert old.redirect == "ns.c.new"
        assert old.deprecation is not None
        assert old.deprecation.removal_version == "3.0"
        noop = routing["event_filter"]["noop"]
        assert noop.deprecation is None
        assert noop.tombstone is not None

    @pytest.mark.parametrize(
        "data",
        [
            b"",
            b"- a\n",
            b"other: 1\n",
            b"plugin_routing: {event_source: [a]}\n",
        ],
    )
    def test_refused_file_read_as_empty(self, data: bytes) -> None:
        # ansible-rulebook refuses the file.
        assert not any(EdaRuntime.parse(data).plugin_routing.values())

    def test_refused_entry_left_out(self) -> None:
        data = (
            b"plugin_routing: {event_source: {a: {redirect: 1}, b: {redirect: ns.c.b}, c: null, "
            b"d: {deprecation: soon}}}\n"
        )

        assert list(EdaRuntime.parse(data).plugin_routing["event_source"]) == ["b"]


@pytest.mark.parametrize("name", ["community.general", "a.b", "ns.async_tasks"])
def test_check_name_valid(name: str) -> None:
    check_name(name)


@pytest.mark.parametrize(
    ("name", "message"),
    [
        ("nope", "expected namespace.name"),
        ("Ns.coll", "collection names are lowercase"),
        ("ns.class", "isn't a keyword"),
        ("a.b.c", "expected namespace.name"),
    ],
)
def test_check_name_refused(name: str, message: str) -> None:
    with pytest.raises(ValueError, match=message):
        check_name(name)
