# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Extraction of a collection's plugins, ported with alai-plugin-extractor's tests. Each test's files are written to a
directory and extracted from there."""

from __future__ import annotations

import collections.abc as _c
import json
import pathlib
import tempfile
import typing as _t

import pytest

from alai_core import extract as ext
from alai_core.collection import MetadataError
from alai_core.yaml import YamlError

Files = _c.Mapping[str, str | bytes]
"""A collection's files, by path."""


def MemSource(files: Files) -> Files:
    return files


def run(files: Files, *, docs: bool = False) -> ext.Collection:
    """Extract a collection of ``files``, by path, each str or bytes."""
    with tempfile.TemporaryDirectory() as root:
        for name, content in files.items():
            path = pathlib.Path(root, name)
            path.parent.mkdir(parents=True, exist_ok=True)
            _ = path.write_bytes(content.encode() if isinstance(content, str) else content)
        return ext.extract(root, docs=docs)


def manifest(info: _c.Mapping[str, object]) -> str:
    """A MANIFEST.json as ansible-galaxy builds it, with ``info`` as its collection_info."""
    files = {"name": "FILES.json", "ftype": "file", "chksum_type": "sha256", "chksum_sha256": "0" * 64, "format": 1}
    return json.dumps({"collection_info": info, "file_manifest_file": files, "format": 1})


def routing(result: ext.Collection) -> dict[str, dict[str, object]]:
    return {f"{r.type}:{r.name}": r.entry for r in result.routing}


def issues(result: ext.Collection, kind: str) -> list[str]:
    return [i.path for i in result.issues if i.kind == kind]


def doc_counts(result: ext.Collection) -> tuple[int, int]:
    """(found, missing) short descriptions, playbooks aside as they have no docs."""
    described = [p.short_description is not None for p in result.plugins if p.type != "playbook"]
    return sum(described), len(described) - sum(described)


def names(result: ext.Collection, ptype: str) -> list[str]:
    return sorted(p.name for p in result.plugins if p.type == ptype)


def test_filters_from_local_variable_and_update():
    src = MemSource(
        {
            "plugins/filter/time.py": """
class FilterModule:
    def filters(self):
        filters = {"to_seconds": a, "to_minutes": b}
        filters.update({"to_hours": c})
        filters["to_days"] = d
        return filters
"""
        }
    )
    assert names(run(src), "filter") == ["to_days", "to_hours", "to_minutes", "to_seconds"]


def test_dict_call_and_direct_return():
    src = MemSource(
        {
            "plugins/filter/a.py": "class FilterModule:\n    def filters(self):\n        return dict(one=f, two=g)\n",
            "plugins/test/b.py": "class TestModule:\n    def tests(self):\n        return {'is_thing': t}\n",
        }
    )
    result = run(src)
    assert names(result, "filter") == ["one", "two"]
    assert names(result, "test") == ["is_thing"]


def test_dynamic_filters_are_reported_unresolved():
    src = MemSource(
        {
            "plugins/filter/dyn.py": "class FilterModule:\n    def filters(self):\n        return {k: v for k, v in X.items()}\n"
        }
    )
    result = run(src)
    assert names(result, "filter") == []
    assert issues(result, "filter_test_unresolved") == ["plugins/filter/dyn.py"]


def test_modules_docs_roles_playbooks_and_routing():
    src = MemSource(
        {
            "plugins/modules/thing.py": 'DOCUMENTATION = """\nmodule: thing\nshort_description: Manage   things\n"""\n',
            "plugins/modules/win_thing.ps1": "# powershell",
            "plugins/modules/win_thing.yml": "DOCUMENTATION:\n  short_description: Windows things\n",
            "plugins/modules/sub/nested.py": "",
            "plugins/action/only_action.py": "",
            "plugins/action/thing.py": "",
            "plugins/module_utils/helper.py": "",
            "roles/setup/tasks/main.yml": "",
            "roles/setup/meta/argument_specs.yml": "argument_specs:\n  main:\n    short_description: Set it up\n",
            "playbooks/site.yml": "",
            "meta/runtime.yml": "plugin_routing:\n  modules:\n    old_thing:\n      redirect: ns.coll.thing\n",
        }
    )
    result = run(src)
    plugins = {(p.type, p.name): p.short_description for p in result.plugins}
    assert plugins == {
        ("module", "thing"): "Manage things",
        ("module", "win_thing"): "Windows things",
        ("module", "sub.nested"): None,
        ("module", "only_action"): None,
        ("role", "setup"): "Set it up",
        ("playbook", "site"): None,
    }
    assert routing(result) == {"module:old_thing": {"redirect": "ns.coll.thing"}}


def test_python2_code_does_not_break_extraction():
    src = MemSource({"plugins/lookup/old.py": "print 'python 2'\n"})
    result = run(src)
    assert names(result, "lookup") == ["old"]
    assert len(issues(result, "python_unparseable")) == 1


def test_routing_dates_stay_strings():
    src = MemSource(
        {
            "meta/runtime.yml": "plugin_routing:\n  modules:\n    old:\n      deprecation:\n        removal_date: 2024-01-01\n"
        }
    )
    entries = routing(run(src))
    assert entries == {"module:old": {"deprecation": {"removal_date": "2024-01-01"}}}
    json.dumps(entries)


def test_class_attribute_returned_conditionally():
    src = MemSource(
        {
            "plugins/filter/ipv4.py": """
class FilterModule(object):
    filter_map = {"ipv4": _ipv4}

    def filters(self):
        if netaddr:
            return self.filter_map
        else:
            return dict((f, partial(_need_netaddr, f)) for f in self.filter_map)
"""
        }
    )
    assert names(run(src), "filter") == ["ipv4"]


def test_module_level_dict_returned_by_name():
    src = MemSource(
        {
            "plugins/test/t.py": "TESTS = {'a': x, 'b': y}\nclass TestModule:\n    def tests(self):\n        return TESTS\n"
        }
    )
    assert names(run(src), "test") == ["a", "b"]


def descriptions(result: ext.Collection, ptype: str = "module") -> dict[str, str | None]:
    return {p.name: p.short_description for p in result.plugins if p.type == ptype}


# -- Python source edge cases -------------------------------------------------


def test_invalid_utf8_in_comment_still_parses():
    # Seen in h3c_bmc.hdm: a GBK comment in a file declaring UTF-8. Python 3.15
    # rejects it where 3.14 ignored invalid bytes in comments.
    src = MemSource(
        {
            "plugins/modules/redfish.py": b"# -*- coding: utf-8 -*-\n# Des: \xca\xca\xc5\xe4 G6\r\n"
            + b'DOCUMENTATION = "short_description: Redfish"\n',
        }
    )
    result = run(src)
    assert descriptions(result) == {"redfish": "Redfish"}
    assert len(issues(result, "python_unparseable")) == 0


def test_invalid_utf8_in_string_is_replaced():
    # A legacy-encoded byte in a string with no coding declaration fails on
    # every Python version, unlike one in a comment, which only 3.15 rejects.
    src = MemSource({"plugins/modules/legacy.py": b'DOCUMENTATION = "short_description: Caf\xe9"\n'})
    result = run(src)
    assert descriptions(result) == {"legacy": "Caf�"}
    assert len(issues(result, "python_unparseable")) == 0


def test_declared_source_encoding_is_honoured():
    src = MemSource(
        {
            "plugins/modules/latin.py": b"# -*- coding: latin-1 -*-\n"
            + b'DOCUMENTATION = "short_description: Caf\xe9"\n',
            "plugins/modules/bom.py": b'\xef\xbb\xbfDOCUMENTATION = "short_description: BOM"\n',
        }
    )
    assert descriptions(run(src)) == {"bom": "BOM", "latin": "Café"}


@pytest.mark.parametrize(
    "source",
    [
        pytest.param(b"print 'python 2'\n", id="python2-print"),
        pytest.param(b"except Exception, e:\n", id="python2-except"),
        pytest.param(b'DOCUMENTATION = """\nshort_description: never closed\n', id="unterminated-string"),
        pytest.param(b"def f():\nreturn 1\n", id="bad-indentation"),
        pytest.param(b"x = 1\x00\n", id="null-byte"),
    ],
)
def test_unparseable_module_is_listed_without_description(source: bytes) -> None:
    result = run(MemSource({"plugins/modules/broken.py": source}))
    # Found in the text as ansible-doc -l finds it, if it's there at all.
    expected = "never closed" if b"never closed" in source else None
    assert descriptions(result) == {"broken": expected}
    assert len(issues(result, "python_unparseable")) == 1
    assert doc_counts(result)[1] == (0 if expected else 1)


# -- DOCUMENTATION and YAML ---------------------------------------------------


@pytest.mark.parametrize(
    ("doc", "expected"),
    [
        pytest.param("short_description: Plain", "Plain", id="plain"),
        pytest.param("short_description: >\n  Folded\n    across   lines\n", "Folded across lines", id="whitespace"),
        pytest.param("short_description: '   '", None, id="blank"),
        pytest.param("short_description: 42", None, id="not-a-string"),
        pytest.param("- short_description: In a list", None, id="list"),
        pytest.param("just a string", None, id="scalar"),
        pytest.param("short_description: [unclosed", None, id="invalid-yaml"),
        pytest.param("!!python/object/apply:os.system ['echo unsafe']", None, id="unsafe-tag"),
        pytest.param("short_description: !!python/name:os.system", None, id="unsafe-nested-tag"),
    ],
)
def test_module_documentation_shapes(doc: str, expected: object) -> None:
    src = MemSource({"plugins/modules/m.py": f"DOCUMENTATION = {doc!r}\n"})
    assert descriptions(run(src)) == {"m": expected}


@pytest.mark.parametrize(
    "source",
    [
        pytest.param('DOCUMENTATION = "short_description: " + "Joined"\n', id="concatenation"),
        pytest.param('def f():\n    DOCUMENTATION = "short_description: Local"\n', id="not-module-level"),
        # Only a plain assignment: ansible-core doesn't read an annotated one.
        pytest.param('DOCUMENTATION: str = "short_description: Annotated"\n', id="annotated"),
    ],
)
def test_documentation_must_be_a_module_level_string_constant(source: str) -> None:
    # Anything else would need evaluating the module, which extraction never does.
    assert descriptions(run(MemSource({"plugins/modules/m.py": source}))) == {"m": None}


@pytest.mark.parametrize(
    "sidecar",
    [
        pytest.param("DOCUMENTATION: [unclosed", id="invalid-yaml"),
        pytest.param("- DOCUMENTATION: {}", id="not-a-dict"),
        pytest.param("EXAMPLES: ''", id="no-documentation"),
    ],
)
def test_bad_sidecar_documentation_is_ignored(sidecar: str) -> None:
    src = MemSource({"plugins/modules/win.ps1": "", "plugins/modules/win.yml": sidecar})
    assert descriptions(run(src)) == {"win": None}


def test_yaml_sidecar_extension_and_python_doc_wins():
    src = MemSource(
        {
            "plugins/modules/both.py": 'DOCUMENTATION = "short_description: From Python"\n',
            "plugins/modules/both.yml": "DOCUMENTATION:\n  short_description: From sidecar\n",
            "plugins/modules/side.ps1": "",
            "plugins/modules/side.yaml": "DOCUMENTATION:\n  short_description: From .yaml\n",
        }
    )
    assert descriptions(run(src)) == {"both": "From Python", "side": "From .yaml"}


# -- collection layout --------------------------------------------------------


def test_files_that_are_not_plugins_are_ignored():
    src = MemSource(
        {
            "plugins/modules/__init__.py": "",
            "plugins/modules/sub/__init__.py": "",
            "plugins/modules/README.md": "",
            "plugins/modules/notes.txt": "",
            "plugins/modules/a/b/deep.py": "",
            "plugins/action/__init__.py": "",
            "plugins/doc_fragments/frag.py": "",
            "plugins/module_utils/util.py": "",
            "plugins/lookup/data.json": "",
            "plugins/lookup/__init__.py": "",
            "plugins/lookup/real.py": "",
            "playbooks/nested/deep.yml": "",
            "playbooks/notes.md": "",
            "playbooks/site.yaml": "",
            "docs/plugins/modules/fake.py": "",
        }
    )
    plugins = sorted((p.type, p.name) for p in run(src).plugins)
    assert plugins == [("lookup", "real"), ("module", "a.b.deep"), ("playbook", "site")]


def test_filters_in_subdirectories_are_prefixed_and_use_sidecars():
    src = MemSource(
        {
            "plugins/filter/net/ip.py": "class FilterModule:\n    def filters(self):\n        return {'ipaddr': f}\n",
            "plugins/filter/net/ipaddr.yml": "DOCUMENTATION:\n  short_description: IP addresses\n",
            # Beside the file providing it, not in plugins/filter/.
            "plugins/filter/sub/s.py": "class FilterModule:\n    def filters(self):\n        return {'other': f}\n",
            "plugins/filter/other.yml": "DOCUMENTATION:\n  short_description: Not beside it\n",
        }
    )
    assert descriptions(run(src), "filter") == {"net.ipaddr": "IP addresses", "sub.other": None}


@pytest.mark.parametrize(
    "body",
    [
        pytest.param("return dict(**OTHER)", id="dict-kwargs-spread"),
        pytest.param("return {**OTHER, 'a': f}", id="dict-spread"),
        pytest.param("return {name(): f}", id="computed-key"),
        pytest.param("return dict(OTHER)", id="dict-positional"),
        pytest.param("return build()", id="call"),
    ],
)
def test_unresolvable_filter_names(body: str) -> None:
    src = MemSource({"plugins/filter/f.py": f"class FilterModule:\n    def filters(self):\n        {body}\n"})
    result = run(src)
    assert names(result, "filter") == []
    assert issues(result, "filter_test_unresolved") == ["plugins/filter/f.py"]


def test_filter_file_without_filter_module_class_is_unresolved():
    src = MemSource({"plugins/filter/f.py": "class Filters:\n    def filters(self):\n        return {'a': f}\n"})
    assert issues(run(src), "filter_test_unresolved") == ["plugins/filter/f.py"]


def test_filter_returns_inside_try_and_for_are_merged():
    src = MemSource(
        {
            "plugins/filter/f.py": """
class FilterModule:
    def filters(self):
        try:
            import thing
        except ImportError:
            return {"fallback": f}
        else:
            for _ in range(1):
                return {"primary": g}
"""
        }
    )
    assert names(run(src), "filter") == ["fallback", "primary"]


def test_role_descriptions():
    src = MemSource(
        {
            "roles/main_meta/tasks/main.yml": "",
            "roles/main_meta/meta/main.yml": "argument_specs:\n  main:\n    short_description: From main.yml\n",
            "roles/no_meta/tasks/main.yml": "",
            "roles/bad_meta/meta/argument_specs.yml": "argument_specs: [not, a, dict]\n",
            "roles/no_main/meta/argument_specs.yml": "argument_specs:\n  other:\n    short_description: Not main\n",
            "roles/shallow.yml": "",
        }
    )
    assert descriptions(run(src), "role") == {
        "bad_meta": None,
        "main_meta": "From main.yml",
        "no_main": None,
        "no_meta": None,
    }


@pytest.mark.parametrize(
    ("files", "expected"),
    [
        pytest.param({"argument_specs.yaml": "argument_specs: {main: {short_description: Yaml}}"}, "Yaml", id="yaml"),
        pytest.param(
            {"argument_specs.json": '{"argument_specs": {"main": {"short_description": "Json"}}}'}, "Json", id="json"
        ),
        pytest.param(
            {"main.yaml": "argument_specs: {main: {short_description: Main yaml}}"}, "Main yaml", id="main-yaml"
        ),
        pytest.param(
            {"main.json": '{"argument_specs": {"main": {"short_description": "Main json"}}}'},
            "Main json",
            id="main-json",
        ),
        # In ansible-core's order: argument_specs.yml, .yaml, .json, then main.yml, .yaml, .json.
        pytest.param(
            {
                "argument_specs.yaml": "argument_specs: {main: {short_description: Second}}",
                "argument_specs.yml": "argument_specs: {main: {short_description: First}}",
            },
            "First",
            id="yml-before-yaml",
        ),
        pytest.param(
            {
                "main.yml": "argument_specs: {main: {short_description: Main}}",
                "argument_specs.json": '{"argument_specs": {"main": {"short_description": "Spec"}}}',
            },
            "Spec",
            id="argument-specs-before-main",
        ),
        # Only the first file that exists is read, so a spec in a later one isn't used.
        pytest.param(
            {
                "argument_specs.yml": "galaxy_info: {}",
                "main.yml": "argument_specs: {main: {short_description: Not used}}",
            },
            None,
            id="first-file-without-specs",
        ),
        pytest.param(
            {"argument_specs.yml": "", "main.yml": "argument_specs: {main: {short_description: Not used}}"},
            None,
            id="first-file-empty",
        ),
    ],
)
def test_role_argument_spec_files(files: dict[str, str], expected: str | None) -> None:
    result = run(MemSource({f"roles/r/meta/{name}": content for name, content in files.items()}), docs=True)
    assert descriptions(result, "role") == {"r": expected}


def test_role_entry_points_kept_as_written() -> None:
    # Each entry point as ansible-doc shows it, examples inside it: roles have no EXAMPLES or RETURN of their own.
    spec = """
argument_specs:
  main:
    short_description: Main
    description: [One, Two]
    version_added: 1.0.0
    author: [Me]
    notes: [A note]
    seealso: [{module: ns.coll.m}]
    examples: |
      - ansible.builtin.include_role: {name: ns.coll.r}
    options:
      name: {type: str, required: true, description: The name}
  other:
    short_description: Other
"""
    docs = plugin_docs(run(MemSource({"roles/r/meta/argument_specs.yml": spec}), docs=True), "role")["r"]
    assert docs.entry_points is not None
    assert list(docs.entry_points) == ["main", "other"]
    main = _t.cast(dict[str, object], docs.entry_points["main"])
    assert main["examples"] == "- ansible.builtin.include_role: {name: ns.coll.r}\n"
    assert (docs.documentation, docs.examples, docs.returns, docs.fragments) == (None, None, None, ())


# -- collection metadata ----------------------------------------------------------


MANIFEST = {
    "collection_info": {
        "namespace": "ns",
        "name": "coll",
        "version": "1.0.0",
        "dependencies": {"ns.other": ">=1.0.0", "ns.bad": 1},
    }
}


@pytest.mark.parametrize(
    ("files", "expected"),
    [
        pytest.param(
            {"MANIFEST.json": manifest(MANIFEST["collection_info"])},
            ("ns", "coll", "1.0.0", {"ns.other": ">=1.0.0"}),
            id="manifest",
        ),
        # The values aren't checked: one that isn't a string is left out.
        pytest.param(
            {"MANIFEST.json": manifest({"name": 1, "dependencies": []})},
            (None, None, None, {}),
            id="wrong-types",
        ),
        pytest.param(
            {"galaxy.yml": "namespace: ns\nname: src\nversion: 2.0.0\ndependencies: {ns.other: '*'}\n"},
            ("ns", "src", "2.0.0", {"ns.other": "*"}),
            id="source-checkout",
        ),
        pytest.param(
            {"MANIFEST.json": manifest({}), "galaxy.yml": "namespace: ns\n"}, (None, None, None, {}), id="manifest-wins"
        ),
        pytest.param({}, (None, None, None, {}), id="neither"),
    ],
)
def test_collection_metadata(files: Files, expected: tuple[object, ...]) -> None:
    result = run(MemSource(files))
    assert (result.namespace, result.name, result.version, result.dependencies) == expected


@pytest.mark.parametrize(
    ("files", "error", "message"),
    [
        pytest.param({"MANIFEST.json": b"{not json"}, ValueError, "not valid JSON", id="invalid-json"),
        pytest.param({"MANIFEST.json": b"[1, 2]"}, ValueError, "not a JSON object", id="not-an-object"),
        pytest.param(
            {"MANIFEST.json": b'{"collection_info": "ns.coll"}'},
            MetadataError,
            "collection_info: expected an object",
            id="info-not-an-object",
        ),
        # Names alone, as ansible-galaxy never builds it.
        pytest.param(
            {"MANIFEST.json": json.dumps({"collection_info": {"namespace": "ns", "name": "coll"}})},
            MetadataError,
            "file_manifest_file: expected an object",
            id="not-as-built",
        ),
        pytest.param({"galaxy.yml": "a: ["}, YamlError, r"galaxy\.yml: ", id="galaxy-not-yaml"),
        pytest.param({"galaxy.yml": "- a\n"}, MetadataError, "expected a mapping", id="galaxy-not-a-mapping"),
    ],
)
def test_collection_metadata_refused(files: Files, error: type[Exception], message: str) -> None:
    # Each message starts with the file, by its path in the collection.
    name = next(iter(files))
    with pytest.raises(error, match=message) as e:
        _ = run(MemSource(files))

    # The path is the system's, with backslashes on Windows, so only the file's name is compared.
    assert pathlib.Path(str(e.value).split(": ", 1)[0]).name == name


@pytest.mark.parametrize(
    ("runtime", "expected"),
    [
        pytest.param("requires_ansible: '>=2.16'\n", ">=2.16", id="set"),
        # Every scalar is a string, as ansible-core reads the file.
        pytest.param("requires_ansible: 2.16\n", "2.16", id="unquoted"),
        pytest.param("requires_ansible: [a]\n", None, id="not-a-string"),
        pytest.param("plugin_routing: {}\n", None, id="missing"),
    ],
)
def test_requires_ansible(runtime: str, expected: str | None) -> None:
    assert run(MemSource({"meta/runtime.yml": runtime})).requires_ansible == expected


# -- meta/runtime.yml -----------------------------------------------------------


@pytest.mark.parametrize(
    "runtime",
    [
        pytest.param("plugin_routing: [a, b]\n", id="routing-not-a-dict"),
        pytest.param("plugin_routing:\n  modules: [a]\n", id="entries-not-a-dict"),
        pytest.param("plugin_routing:\n  modules:\n    a: redirect\n", id="entry-not-a-dict"),
        pytest.param("plugin_routing:\n  modules:\n    a: {}\n", id="empty-entry"),
        pytest.param("plugin_routing:\n  doc_fragments:\n    a: {redirect: x.y.z}\n", id="unindexed-type"),
        pytest.param("plugin_routing: {modules: [unclosed\n", id="invalid-yaml"),
        pytest.param("- not a mapping\n", id="not-a-mapping"),
    ],
)
def test_malformed_runtime_routing_is_skipped(runtime: str) -> None:
    assert routing(run(MemSource({"meta/runtime.yml": runtime}))) == {}


def test_runtime_yaml_extension_and_routing_types():
    runtime = """
plugin_routing:
  modules:
    old_mod: {redirect: ns.coll.new_mod}
  filter:
    old_filter: {tombstone: {removal_version: 2.0.0}}
  lookup:
    old_lookup: {deprecation: {removal_date: 2025-06-01}}
  action:
    old_mod: {redirect: ns.coll.new_mod}
  terminal:
    old_os: {redirect: ns.coll.new_os}
  module_utils:
    helper: {redirect: ns.coll.other}
"""
    entries = routing(run(MemSource({"meta/runtime.yaml": runtime})))
    assert entries == {
        "action:old_mod": {"redirect": "ns.coll.new_mod"},
        "filter:old_filter": {"tombstone": {"removal_version": "2.0.0"}},
        "lookup:old_lookup": {"deprecation": {"removal_date": "2025-06-01"}},
        "module:old_mod": {"redirect": "ns.coll.new_mod"},
        "terminal:old_os": {"redirect": "ns.coll.new_os"},
    }


def test_terminal_plugins():
    # Chosen by a host's ansible_network_os rather than named in a play, and documented by nothing ansible-doc reads.
    src = MemSource(
        {
            "plugins/terminal/__init__.py": "",
            "plugins/terminal/ios.py": "class TerminalModule: pass\n",
            "plugins/terminal/eos.py": 'DOCUMENTATION = "short_description: EOS"\n',
        }
    )
    assert {(p.type, p.name): p.short_description for p in run(src).plugins} == {
        ("terminal", "eos"): "EOS",
        ("terminal", "ios"): None,
    }


# -- sources --------------------------------------------------------------------


# -- filter name resolution: mutations --------------------------------------------


def _filters(body: str) -> Files:
    lines = "\n".join(f"        {line}" for line in body.strip().splitlines())
    return MemSource({"plugins/filter/f.py": f"class FilterModule:\n    def filters(self):\n{lines}\n"})


@pytest.mark.parametrize(
    ("body", "expected"),
    [
        pytest.param("x = {'a': f}\nx['b'] = g\nreturn x", ["a", "b"], id="literal-subscript"),
        pytest.param("x = {'a': f}\nx |= {'b': g}\nreturn x", ["a", "b"], id="or-assign"),
        pytest.param("x = {'a': f}\nx.update(b=g, c=h)\nreturn x", ["a", "b", "c"], id="update-keywords"),
        pytest.param("x = {'a': f}\nx.update({'b': g}, c=h)\nreturn x", ["a", "b", "c"], id="update-mixed"),
        pytest.param("self.x = {'a': f}\nself.x['b'] = g\nreturn self.x", ["a", "b"], id="self-attribute"),
        pytest.param("x = {'a': f}\nother.attr[NAME] = 1\nreturn x", ["a"], id="unrelated-subscript"),
    ],
)
def test_filter_dict_mutations_that_resolve(body: str, expected: list[str]) -> None:
    assert names(run(_filters(body)), "filter") == expected


@pytest.mark.parametrize(
    "body",
    [
        pytest.param("x = {'a': f}\nx[NAME] = g\nreturn x", id="computed-subscript"),
        pytest.param("x = {'a': f}\nx[1] = g\nreturn x", id="non-string-subscript"),
        pytest.param("self.x = {'a': f}\nself.x[NAME] = g\nreturn self.x", id="computed-self-subscript"),
        pytest.param("x = {'a': f}\nx |= OTHER\nreturn x", id="or-assign-unknown"),
        pytest.param("x = {'a': f}\nx += OTHER\nreturn x", id="other-augmented-op"),
        pytest.param("x = {'a': f}\nx.update(**OTHER)\nreturn x", id="update-kwargs-spread"),
        pytest.param("x = {'a': f}\nx.update(OTHER)\nreturn x", id="update-unknown"),
        pytest.param("x = {'a': f}\nx.update(A, B)\nreturn x", id="update-two-args"),
    ],
)
def test_filter_dict_mutations_that_cannot_resolve(body: str) -> None:
    # A name the extractor can't see must make the file unresolved, never be
    # silently dropped from an otherwise resolved list.
    result = run(_filters(body))
    assert names(result, "filter") == []
    assert issues(result, "filter_test_unresolved") == ["plugins/filter/f.py"]


def test_filter_module_without_filters_method_is_unresolved():
    src = MemSource({"plugins/filter/f.py": "class FilterModule:\n    def other(self):\n        return {'a': f}\n"})
    assert issues(run(src), "filter_test_unresolved") == ["plugins/filter/f.py"]


def test_unparseable_filter_file_is_unresolved():
    result = run(MemSource({"plugins/filter/f.py": "print 'python 2'\n"}))
    assert names(result, "filter") == []
    assert len(issues(result, "python_unparseable")) == 1
    assert issues(result, "filter_test_unresolved") == ["plugins/filter/f.py"]


# -- modules backed by PowerShell or action plugins -------------------------------


def test_powershell_module_with_python_doc_stub_is_counted_once():
    src = MemSource(
        {
            "plugins/modules/win_thing.ps1": "# implementation",
            "plugins/modules/win_thing.py": 'DOCUMENTATION = "short_description: Windows thing"\n',
            "plugins/modules/win_broken.ps1": "# implementation",
            "plugins/modules/win_broken.py": "print 'python 2'\n",
        }
    )
    result = run(src)
    assert descriptions(result) == {"win_broken": None, "win_thing": "Windows thing"}
    assert len(issues(result, "python_unparseable")) == 1
    assert doc_counts(result) == (1, 1)


def test_action_only_module_docs_from_module_sidecar():
    # e.g. capable.core212: the implementation is an action plugin and the docs
    # are only a plugins/modules/<name>.yml sidecar.
    src = MemSource(
        {
            "plugins/action/show.py": "class ActionModule: pass\n",
            "plugins/modules/show.yml": "DOCUMENTATION:\n  short_description: From sidecar\n",
            "plugins/action/net/ping.py": "",
            "plugins/modules/net/ping.yaml": "DOCUMENTATION:\n  short_description: Nested sidecar\n",
            "plugins/action/own.py": 'DOCUMENTATION = "short_description: From action"\n',
            "plugins/modules/own.yml": "DOCUMENTATION:\n  short_description: From sidecar\n",
            "plugins/action/undocumented.py": "",
            "plugins/action/broken.py": "print 'python 2'\n",
            "plugins/modules/broken.yml": "DOCUMENTATION:\n  short_description: Still documented\n",
        }
    )
    result = run(src)
    assert descriptions(result) == {
        "broken": "Still documented",
        "net.ping": "Nested sidecar",
        "own": "From action",
        "show": "From sidecar",
        "undocumented": None,
    }
    assert len(issues(result, "python_unparseable")) == 1
    # The sidecar alone isn't a module, so nothing is listed twice.
    assert doc_counts(result) == (4, 1)


def test_action_with_module_uses_module_docs():
    src = MemSource(
        {
            "plugins/action/win_copy.py": 'DOCUMENTATION = "short_description: Action"\n',
            "plugins/modules/win_copy.ps1": "",
            "plugins/modules/win_copy.py": 'DOCUMENTATION = "short_description: Module"\n',
        }
    )
    assert descriptions(run(src)) == {"win_copy": "Module"}


def test_playbook_with_both_extensions_is_listed_once():
    src = MemSource({"playbooks/site.yml": "", "playbooks/site.yaml": ""})
    result = run(src)
    assert [(p.type, p.name) for p in result.plugins] == [("playbook", "site")]
    assert doc_counts(result) == (0, 0)


def test_filter_defined_in_two_files_is_listed_once():
    body = "class FilterModule:\n    def filters(self):\n        return {'shared': f}\n"
    src = MemSource(
        {
            "plugins/filter/a.py": body,
            "plugins/filter/b.py": body,
            "plugins/filter/shared.yml": "DOCUMENTATION:\n  short_description: Shared\n",
        }
    )
    result = run(src)
    assert descriptions(result, "filter") == {"shared": "Shared"}
    assert doc_counts(result) == (1, 0)


# -- public entry points ------------------------------------------------------------


def test_extract_a_directory(tmp_path: pathlib.Path) -> None:
    root = tmp_path / "coll"
    for name, content in {
        "MANIFEST.json": manifest({"namespace": "ns", "name": "coll", "version": "1.0.0"}),
        "plugins/modules/thing.py": 'DOCUMENTATION = "short_description: Thing"\n',
    }.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        _ = (root / name).write_text(content)

    for path in (root, str(root)):
        result = ext.extract(path)
        assert (result.namespace, result.name, result.version) == ("ns", "coll", "1.0.0")
        assert [(p.type, p.name, p.short_description) for p in result.plugins] == [("module", "thing", "Thing")]


def test_extract_errors(tmp_path: pathlib.Path) -> None:
    with pytest.raises(FileNotFoundError):
        _ = ext.extract(tmp_path / "missing")
    tarball = tmp_path / "ns-coll-1.0.0.tar.gz"
    _ = tarball.write_bytes(b"")
    with pytest.raises(NotADirectoryError, match="extract a tarball first"):
        _ = ext.extract(tarball)


def test_directory_is_read_only_from_inside(tmp_path: pathlib.Path) -> None:
    root = tmp_path / "coll"
    modules = root / "plugins" / "modules"
    modules.mkdir(parents=True)
    _ = (modules / "real.py").write_text('DOCUMENTATION = "short_description: Real"\n')
    (modules / "alias.py").symlink_to("real.py")
    (modules / "dangling.py").symlink_to("missing.py")
    _ = (tmp_path / "secret.py").write_text('DOCUMENTATION = "short_description: Secret"\n')
    (modules / "escape.py").symlink_to(tmp_path / "secret.py")
    # A link to a directory isn't walked, even one inside the collection.
    (root / "plugins" / "lookup").symlink_to(modules, target_is_directory=True)

    result = ext.extract(root)

    # A dangling link isn't a file; one leading outside is, but isn't read.
    assert descriptions(result) == {"alias": "Real", "escape": None, "real": "Real"}
    assert names(result, "lookup") == []


# -- full docs ------------------------------------------------------------------


def plugin_docs(result: ext.Collection, ptype: str = "module") -> dict[str, ext.PluginDocs]:
    """Each plugin's docs, without the file they came from, which ``docs_paths`` gives."""
    return {p.name: without_path(p.docs) for p in result.plugins if p.type == ptype and p.docs is not None}


def without_path(docs: ext.PluginDocs) -> ext.PluginDocs:
    return ext.PluginDocs(
        documentation=docs.documentation,
        examples=docs.examples,
        returns=docs.returns,
        entry_points=docs.entry_points,
        fragments=docs.fragments,
        return_fragments=docs.return_fragments,
    )


def docs_paths(result: ext.Collection) -> dict[tuple[str, str], str | None]:
    """The file each plugin's docs were read from, by type and name."""
    return {(p.type, p.name): p.docs.path if p.docs else None for p in result.plugins}


def test_docs_are_off_by_default():
    result = run(
        MemSource(
            {
                "plugins/modules/m.py": 'DOCUMENTATION = "short_description: M"\n',
                "plugins/doc_fragments/f.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "options: {}"\n',
            }
        )
    )
    assert [(p.type, p.name, p.short_description, p.docs) for p in result.plugins] == [("module", "m", "M", None)]
    assert result.fragments == {}


def test_module_docs_are_raw():
    module = '''
DOCUMENTATION = """
short_description: Thing
extends_documentation_fragment:
- ns.other.auth
- ns.coll.common.facts
options:
  name: {type: str}
"""
EXAMPLES = """
- ns.coll.thing:
    name: x
"""
RETURN = """
extends_documentation_fragment: ns.other.results, files
changed: {type: bool}
"""
'''
    src = MemSource({"plugins/modules/thing.py": module})
    result = run(src, docs=True)
    assert plugin_docs(result) == {
        # Fragments are listed for the caller, who may have ns.other.
        "thing": ext.PluginDocs(
            documentation={"short_description": "Thing", "options": {"name": {"type": "str"}}},
            examples="\n- ns.coll.thing:\n    name: x\n",
            returns={"changed": {"type": "bool"}},
            fragments=("ns.other.auth", "ns.coll.common.facts"),
            return_fragments=("ns.other.results", "files"),
        )
    }
    # Asking for docs doesn't change anything else.
    without = run(src)
    assert [(p.type, p.name, p.short_description) for p in result.plugins] == [
        (p.type, p.name, p.short_description) for p in without.plugins
    ]


@pytest.mark.parametrize(
    ("module", "expected"),
    [
        pytest.param('EXAMPLES = "  "\nRETURN = ""\n', None, id="blank-examples-empty-return"),
        pytest.param('RETURN = "- a list"\n', None, id="return-not-a-mapping"),
        # Without DOCUMENTATION there are no docs, as ansible-doc has none to show.
        pytest.param('EXAMPLES = "- x: 1"\n', None, id="examples-only"),
        pytest.param("DOCUMENTATION: str\n", None, id="annotation-without-value"),
        # e.g. jomrr.ca's fragments, which ansible-core doesn't read.
        pytest.param('DOCUMENTATION: str = "a: 1"\n', None, id="annotated"),
        pytest.param('x.DOCUMENTATION = "a: 1"\n', None, id="attribute-target"),
        pytest.param(
            'DOCUMENTATION = "a: 1"\nDOCUMENTATION = "a: 2"\n',
            ext.PluginDocs(documentation={"a": 2}),
            id="last-constant-wins",
        ),
        pytest.param(
            # The last assignment wins, so one ansible-core can't read is replaced, though ansible-core fails on it.
            'DOCUMENTATION = build()\nDOCUMENTATION = "a: 2"\n',
            ext.PluginDocs(documentation={"a": 2}),
            id="call-replaced",
        ),
        pytest.param(
            "DOCUMENTATION = \"extends_documentation_fragment: [a, 1, ' ', {b: c}]\"\n",
            ext.PluginDocs(documentation={}, fragments=("a",)),
            id="fragment-list-junk",
        ),
        pytest.param(
            'DOCUMENTATION = "extends_documentation_fragment: {a: b}"\n',
            ext.PluginDocs(documentation={}),
            id="fragments-not-a-list",
        ),
    ],
)
def test_module_doc_variants(module: str, expected: ext.PluginDocs | None) -> None:
    assert plugin_docs(run(MemSource({"plugins/modules/m.py": module}), docs=True)).get("m") == expected


def test_doc_sources_in_priority_order():
    src = MemSource(
        {
            # Python docs win, and the sidecar isn't needed.
            "plugins/modules/both.py": 'DOCUMENTATION = "short_description: Py"\n',
            "plugins/modules/both.yml": "DOCUMENTATION:\n  short_description: Sidecar\n",
            # DOCUMENTATION without a short_description is still the docs, and the
            # sidecar isn't read.
            "plugins/modules/split.py": 'DOCUMENTATION = "options: {}"\n',
            "plugins/modules/split.yml": "DOCUMENTATION:\n  short_description: Sidecar\n",
            # A .ps1 module documented only by a sidecar.
            "plugins/modules/win.ps1": "",
            "plugins/modules/win.yml": "DOCUMENTATION:\n  short_description: Win\nEXAMPLES: '- win: {}'\n"
            + "RETURN:\n  out: {type: str}\n",
            # Action-only, documented by a .yaml sidecar.
            "plugins/action/act.py": "",
            "plugins/modules/act.yaml": "DOCUMENTATION:\n  short_description: Act\n",
            # Nothing usable anywhere.
            "plugins/modules/none.py": "x = 1\n",
            "plugins/modules/none.yml": "- not a mapping\n",
        }
    )
    result = run(src, docs=True)
    assert descriptions(result) == {"act": "Act", "both": "Py", "none": None, "split": None, "win": "Win"}
    # none.yml isn't a mapping: an error of the whole file, which is still its docs file.
    none = next(p for p in result.plugins if p.name == "none").docs
    assert none is not None and [(e.section, e.message) for e in none.errors] == [(None, "isn't a mapping")]
    assert {k: v for k, v in plugin_docs(result).items() if k != "none"} == {
        "act": ext.PluginDocs(documentation={"short_description": "Act"}),
        "both": ext.PluginDocs(documentation={"short_description": "Py"}),
        "split": ext.PluginDocs(documentation={"options": {}}),
        "win": ext.PluginDocs(
            documentation={"short_description": "Win"}, examples="- win: {}", returns={"out": {"type": "str"}}
        ),
    }


def test_docs_for_filters_roles_and_other_plugins():
    src = MemSource(
        {
            "plugins/filter/f.py": "class FilterModule:\n    def filters(self):\n        return {'a': x, 'b': y}\n",
            "plugins/filter/a.yml": "DOCUMENTATION:\n  short_description: A\nEXAMPLES: 'x | ns.coll.a'\n",
            "plugins/lookup/l.py": 'DOCUMENTATION = "short_description: L"\nRETURN = "_raw: {}"\n',
            "plugins/lookup/broken.py": "print 'python 2'\n",
            "roles/r/meta/argument_specs.yml": "argument_specs:\n  main: {short_description: Main}\n"
            + "  other: {options: {}}\n",
            "playbooks/site.yml": "",
        }
    )
    result = run(src, docs=True)
    assert {(p.type, p.name): p.docs and without_path(p.docs) for p in result.plugins} == {
        ("filter", "a"): ext.PluginDocs(documentation={"short_description": "A"}, examples="x | ns.coll.a"),
        ("filter", "b"): None,
        # Python 2, so an error of the whole file.
        ("lookup", "broken"): ext.PluginDocs(),
        ("lookup", "l"): ext.PluginDocs(documentation={"short_description": "L"}, returns={"_raw": {}}),
        ("playbook", "site"): None,
        ("role", "r"): ext.PluginDocs(entry_points={"main": {"short_description": "Main"}, "other": {"options": {}}}),
    }
    assert descriptions(result, "role") == {"r": "Main"}
    assert docs_paths(result) == {
        ("filter", "a"): "plugins/filter/a.yml",
        ("filter", "b"): None,
        ("lookup", "broken"): "plugins/lookup/broken.py",
        ("lookup", "l"): "plugins/lookup/l.py",
        ("playbook", "site"): None,
        ("role", "r"): "roles/r/meta/argument_specs.yml",
    }


def test_description_of_unreadable_docs_is_the_top_level_one() -> None:
    # An option named short_description before the top-level one isn't taken for it, whether the YAML breaks after
    # the top-level one, read as far as it, or before it, found in the text at the top level only.
    after = "options:\n  short_description:\n    description: An option\nshort_description: After\noptions: [\n"
    before = "options:\n  short_description:\n    description: An option\nbroken: [\nshort_description: Before\n"
    sidecar = "DOCUMENTATION:\n  options:\n    short_description: {}\n  short_description: Sidecar\n  bad: [\n"
    src = MemSource(
        {
            "plugins/modules/after.py": f"DOCUMENTATION = {after!r}\n",
            "plugins/modules/before.py": f"DOCUMENTATION = {before!r}\n",
            "plugins/lookup/side.py": "x = 1\n",
            "plugins/lookup/side.yml": sidecar,
            # Python 2, so only the text: the top-level line, not the option's.
            "plugins/modules/py2.py": 'print "x"\nDOCUMENTATION = """\noptions:\n  short_description: {}\n'
            + 'short_description: Python 2\n"""\n',
        }
    )
    result = run(src)
    assert descriptions(result) == {"after": "After", "before": "Before", "py2": "Python 2"}
    assert descriptions(result, "lookup") == {"side": "Sidecar"}


def test_unreadable_sections_are_errors() -> None:
    # Each section is read on its own: one that can't be read is an error, with its text, and the rest are kept.
    src = MemSource(
        {
            "plugins/modules/bad_return.py": 'DOCUMENTATION = "short_description: Kept"\nRETURN = "a: [unclosed"\n',
            "plugins/modules/bad_doc.py": 'DOCUMENTATION = """\nshort_description: From the text.\noptions: [unclosed\n"""\n'
            + 'EXAMPLES = "- m: {}"\n',
            "plugins/modules/expression.py": 'DOCUMENTATION = "short_description: " + "Joined"\n',
            "plugins/lookup/sidecar.py": "x = 1\n",
            "plugins/lookup/sidecar.yml": "DOCUMENTATION: [unclosed\n  short_description: Sidecar text\n",
        }
    )
    result = run(src, docs=True)

    def errors(name: str) -> list[tuple[str | None, bool]]:
        docs = next(p for p in result.plugins if p.name == name).docs
        assert docs is not None
        return [(e.section, e.text is not None) for e in docs.errors]

    assert errors("bad_return") == [("RETURN", True)]
    assert errors("bad_doc") == [("DOCUMENTATION", True)]
    assert errors("expression") == [("DOCUMENTATION", False)]
    assert errors("sidecar") == [(None, True)]
    docs = plugin_docs(result)
    assert docs["bad_return"] == ext.PluginDocs(documentation={"short_description": "Kept"})
    assert docs["bad_doc"] == ext.PluginDocs(examples="- m: {}")
    # The description is still found in the text, as ansible-doc -l finds it.
    assert descriptions(result) == {"bad_doc": "From the text.", "bad_return": "Kept", "expression": None}
    assert descriptions(result, "lookup") == {"sidecar": "Sidecar text"}
    # Each is an issue, with the section, or for the whole file only the reason.
    found = {i.path: i.detail for i in result.issues if i.kind == "docs_unreadable"}
    assert {path: detail.split(":")[0] for path, detail in found.items() if path.endswith(".py")} == {
        "plugins/modules/bad_doc.py": "DOCUMENTATION",
        "plugins/modules/bad_return.py": "RETURN",
        "plugins/modules/expression.py": "DOCUMENTATION",
    }
    assert found["plugins/lookup/sidecar.yml"].startswith("did not find expected")


def test_without_docs_only_the_description_is_read() -> None:
    # Reading stops at short_description, so what breaks after it, or RETURN, isn't an issue: that's docs' concern.
    # Without one, or with one that isn't text, the whole docs are read as with docs, so the result is the same.
    src = MemSource(
        {
            "plugins/modules/after.py": 'DOCUMENTATION = "short_description: After\\noptions: [unclosed"\n',
            "plugins/modules/bad_return.py": 'DOCUMENTATION = "short_description: Kept"\nRETURN = "a: [unclosed"\n',
            "plugins/modules/number.py": 'DOCUMENTATION = "short_description: 123"\n',
            "plugins/modules/empty.py": 'DOCUMENTATION = ""\n',
            "plugins/modules/empty.yml": "DOCUMENTATION:\n  short_description: Sidecar\n",
            "plugins/lookup/side.py": "x = 1\n",
            "plugins/lookup/side.yml": "DOCUMENTATION:\n  short_description: Sidecar\nRETURN: [unclosed\n",
        }
    )
    expected = {"after": "After", "bad_return": "Kept", "empty": "Sidecar", "number": None}
    result = run(src)
    assert descriptions(result) == expected
    assert descriptions(result, "lookup") == {"side": "Sidecar"}
    assert issues(result, "docs_unreadable") == []
    result = run(src, docs=True)
    assert descriptions(result) == expected
    assert descriptions(result, "lookup") == {"side": "Sidecar"}
    assert sorted(issues(result, "docs_unreadable")) == [
        "plugins/lookup/side.yml",
        "plugins/modules/after.py",
        "plugins/modules/bad_return.py",
    ]


def test_docs_found_as_ansible_doc_finds_them() -> None:
    # Each checked against ansible-doc from ansible-core 2.21.5: the short description, and the file it read.
    src = MemSource(
        {
            # How a Python file's constants are read.
            "plugins/modules/chained.py": 'DOCUMENTATION = OTHER = "short_description: Chained"\n',
            "plugins/modules/bytes.py": 'DOCUMENTATION = b"short_description: Bytes"\n',
            "plugins/modules/last.py": 'DOCUMENTATION = "short_description: First"\nDOCUMENTATION = "short_description: Last"\n',
            "plugins/modules/dict.py": 'DOCUMENTATION = {"short_description": "Dict"}\n',
            "plugins/modules/stops.py": 'DOCUMENTATION = "short_description: Before"\nRETURN = "a" + "b"\n',
            # One file supplies them all: EXAMPLES aren't taken from a file without DOCUMENTATION.
            "plugins/modules/split.py": 'EXAMPLES = "py examples"\n',
            "plugins/modules/split.yml": "DOCUMENTATION:\n  short_description: Sidecar\nEXAMPLES: sidecar examples\n",
            # A .ps1 is resolved first, then the first docs file beside it.
            "plugins/modules/win.ps1": "# ps\n",
            "plugins/modules/win.py": 'DOCUMENTATION = "short_description: Py stub"\n',
            "plugins/modules/win.yml": "DOCUMENTATION:\n  short_description: Yml\n",
            "plugins/modules/winyml.ps1": "# ps\n",
            "plugins/modules/winyml.yml": "DOCUMENTATION:\n  short_description: Yml only\n",
            # Any plugin can have a sidecar.
            "plugins/lookup/side.py": "x = 1\n",
            "plugins/lookup/side.yml": "DOCUMENTATION:\n  short_description: Lookup sidecar\n",
            # A filter's docs: the file providing it, even documenting another filter, then a sidecar.
            "plugins/filter/core.py": 'DOCUMENTATION = "short_description: Core docs\\nname: alpha"\n'
            + "class FilterModule:\n    def filters(self):\n        return {'alpha': f, 'beta': f}\n",
            "plugins/filter/beta.yml": "DOCUMENTATION:\n  short_description: Beta sidecar\n",
            "plugins/filter/plain.py": "class FilterModule:\n    def filters(self):\n        return {'gamma': f}\n",
            "plugins/filter/gamma.yml": "DOCUMENTATION:\n  short_description: Gamma sidecar\n",
            # The last file providing a name provides it.
            "plugins/filter/a_first.py": 'DOCUMENTATION = "short_description: From a_first"\n'
            + "class FilterModule:\n    def filters(self):\n        return {'dup': f}\n",
            "plugins/filter/z_last.py": 'DOCUMENTATION = "short_description: From z_last"\n'
            + "class FilterModule:\n    def filters(self):\n        return {'dup': f}\n",
            # <name>.py is the docs file, though another file provides the filter.
            "plugins/filter/stubbed.py": 'DOCUMENTATION = "short_description: Stub py"\n',
            "plugins/filter/real.py": 'DOCUMENTATION = "short_description: Real provider"\n'
            + "class FilterModule:\n    def filters(self):\n        return {'stubbed': f}\n",
        }
    )
    result = run(src, docs=True)
    found = {(p.type, p.name): (p.short_description, p.docs.path if p.docs else None) for p in result.plugins}
    assert found == {
        ("filter", "alpha"): ("Core docs", "plugins/filter/core.py"),
        ("filter", "beta"): ("Core docs", "plugins/filter/core.py"),
        ("filter", "dup"): ("From z_last", "plugins/filter/z_last.py"),
        ("filter", "gamma"): ("Gamma sidecar", "plugins/filter/gamma.yml"),
        ("filter", "stubbed"): ("Stub py", "plugins/filter/stubbed.py"),
        ("lookup", "side"): ("Lookup sidecar", "plugins/lookup/side.yml"),
        ("module", "bytes"): ("Bytes", "plugins/modules/bytes.py"),
        ("module", "chained"): ("Chained", "plugins/modules/chained.py"),
        ("module", "dict"): ("Dict", "plugins/modules/dict.py"),
        ("module", "last"): ("Last", "plugins/modules/last.py"),
        ("module", "split"): ("Sidecar", "plugins/modules/split.yml"),
        # ansible-doc shows nothing for this one, its RETURN being unreadable; the rest is kept, RETURN an error.
        ("module", "stops"): ("Before", "plugins/modules/stops.py"),
        ("module", "win"): ("Py stub", "plugins/modules/win.py"),
        ("module", "winyml"): ("Yml only", "plugins/modules/winyml.yml"),
    }
    # Borrowed docs say which filter they're for.
    beta = next(p for p in result.plugins if p.name == "beta").docs
    assert beta is not None and beta.documentation is not None and beta.documentation["name"] == "alpha"
    split = next(p for p in result.plugins if p.name == "split").docs
    assert split is not None and split.examples == "sidecar examples"


def test_first_docs_found_win_for_duplicates():
    body = "class FilterModule:\n    def filters(self):\n        return {'shared': f}\n"
    src = MemSource(
        {
            "plugins/filter/a.py": body,
            "plugins/filter/b.py": body,
            "plugins/filter/shared.yml": "DOCUMENTATION:\n  short_description: Shared\n",
        }
    )
    assert plugin_docs(run(src, docs=True), "filter") == {
        "shared": ext.PluginDocs(documentation={"short_description": "Shared"})
    }


# -- doc fragments ----------------------------------------------------------------


def test_doc_fragments_are_collected_raw():
    fragment = '''
class ModuleDocFragment:
    DOCUMENTATION = r"""
options:
  host: {type: str}
"""
    FACTS = "options: {gather: {type: bool}}"
    RETURN = ""
    lowercase = "options: {}"
'''
    src = MemSource(
        {
            "plugins/doc_fragments/auth.py": fragment,
            "plugins/doc_fragments/__init__.py": "",
            "plugins/doc_fragments/net/opts.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "options: {}"\n',
            "plugins/doc_fragments/empty.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "[unclosed"\n',
        }
    )
    result = run(src, docs=True)
    # Fragments aren't plugins.
    assert result.plugins == ()
    assert {name: f.attributes for name, f in result.fragments.items()} == {
        "auth": {
            "DOCUMENTATION": {"options": {"host": {"type": "str"}}},
            "FACTS": {"options": {"gather": {"type": "bool"}}},
        },
        "net.opts": {"DOCUMENTATION": {"options": {}}},
    }
    assert result.issues == ()


@pytest.mark.parametrize(
    ("fragment", "expected"),
    [
        # e.g. dalibo.pglift, whose fragments are generated by a call, and
        # infisical.vault, which uses str.format().
        pytest.param(
            'class ModuleDocFragment:\n    DOCUMENTATION = build()\n    OTHER = "options: {}"\n',
            {"OTHER": {"options": {}}},
            id="call",
        ),
        pytest.param('class Other:\n    DOCUMENTATION = "options: {}"\n', None, id="no-fragment-class"),
        # e.g. amazon.aws's boto3, which sets every attribute in __init__.
        pytest.param(
            "class ModuleDocFragment:\n    def __init__(self):\n        self.DOCUMENTATION = build()\n",
            None,
            id="set-in-method",
        ),
        pytest.param(
            'class ModuleDocFragment:\n    OTHER = "options: {}"\n    async def later(self): ...\n',
            {"OTHER": {"options": {}}},
            id="async-method",
        ),
        pytest.param("class ModuleDocFragment:\n    lower = 'options: {}'\n", None, id="nothing-readable"),
        # e.g. mglantz.demo_collection, an unterminated string.
        pytest.param("print 'python 2'\n", None, id="unparseable"),
    ],
)
def test_unresolved_doc_fragments(fragment: str, expected: dict[str, object] | None) -> None:
    result = run(MemSource({"plugins/doc_fragments/frag.py": fragment}), docs=True)
    assert (f.attributes if (f := result.fragments.get("frag")) else None) == expected
    assert issues(result, "fragment_unresolved") == ["plugins/doc_fragments/frag.py"]


# -- YAML that must stay JSON -------------------------------------------------------


def test_yaml_values_are_json_safe():
    doc = """
short_description: Safe
version_added_date: !!timestamp 2024-01-01
blob: !!binary aGVsbG8=
default: .nan
limit: -.inf
choices: !!set {b, a}
ordered: !!omap [{x: 1}, {y: 2}]
keys: {2: int, 1.5: float, true: bool, null: none}
"""
    docs = plugin_docs(run(MemSource({"plugins/modules/m.py": f"DOCUMENTATION = {doc!r}\n"}), docs=True))["m"]
    assert docs.documentation == {
        "short_description": "Safe",
        "version_added_date": "2024-01-01",
        "blob": "aGVsbG8=",
        # As yaml_loads gives them: .nan and .inf as written, !!omap's mappings as they are.
        "default": ".nan",
        "limit": "-.inf",
        "choices": {"a": None, "b": None},
        "ordered": [{"x": 1}, {"y": 2}],
        "keys": {"2": "int", "1.5": "float", "true": "bool", "null": "none"},
    }
    _ = json.dumps(docs.documentation, allow_nan=False)


def test_routing_explicit_timestamp_and_numeric_name():
    runtime = "plugin_routing:\n  modules:\n    old:\n      deprecation: {removal_date: !!timestamp 2024-01-01}\n"
    runtime += "    123: {redirect: ns.coll.num}\n"
    assert routing(run(MemSource({"meta/runtime.yml": runtime}))) == {
        "module:123": {"redirect": "ns.coll.num"},
        "module:old": {"deprecation": {"removal_date": "2024-01-01"}},
    }


def _alias_bomb(levels: int) -> str:
    # Each level holds ten references to the one before: 10**levels values
    # when expanded, from a few hundred bytes.
    lines = ["l0: &l0 [x, x, x, x, x, x, x, x, x, x]"]
    lines += [f"l{i}: &l{i} [{', '.join([f'*l{i - 1}'] * 10)}]" for i in range(1, levels + 1)]
    return "\n".join(lines) + "\n"


def test_alias_bombs_are_dropped():
    bomb = _alias_bomb(8)
    src = MemSource(
        {
            "plugins/modules/m.py": f"DOCUMENTATION = 'short_description: Kept'\nRETURN = {bomb!r}\n",
            "meta/runtime.yaml": "plugin_routing:\n  modules:\n    old:\n      redirect: x.y.z\n"
            + "".join(f"      {line}\n" for line in bomb.splitlines()),
        }
    )
    result = run(src, docs=True)
    assert descriptions(result) == {"m": "Kept"}
    assert plugin_docs(result) == {"m": ext.PluginDocs(documentation={"short_description": "Kept"})}
    assert routing(result) == {}
    assert [(i.kind, i.path, i.detail) for i in result.issues] == [
        ("value_too_large", "plugins/modules/m.py", "RETURN"),
        ("value_too_large", "meta/runtime.yaml", "module:old"),
    ]


def test_deep_nesting_is_dropped():
    deep = "x: " + "[" * 100 + "]" * 100
    src = MemSource(
        {
            "plugins/modules/m.py": f"DOCUMENTATION = {deep!r}\n",
            "roles/r/meta/argument_specs.yml": f"argument_specs:\n  main: {{{deep}}}\n",
            "plugins/doc_fragments/f.py": f"class ModuleDocFragment:\n    DOCUMENTATION = {deep!r}\n",
        }
    )
    result = run(src, docs=True)
    assert [p.docs for p in result.plugins] == [None, None]
    assert result.fragments == {}
    assert [(i.path, i.detail) for i in result.issues] == [
        ("plugins/doc_fragments/f.py", "DOCUMENTATION"),
        ("plugins/modules/m.py", "DOCUMENTATION"),
        ("roles/r/meta/argument_specs.yml", "argument_specs"),
    ]


def test_doc_fragment_routing_is_separate_and_needs_docs():
    runtime = """
plugin_routing:
  modules:
    old: {redirect: ns.coll.new}
  doc_fragments:
    legacy: {redirect: ns.coll.current}
    gone: {tombstone: {removal_version: 2.0.0}}
    bad: not a mapping
"""
    src = MemSource({"meta/runtime.yml": runtime})
    with_docs = run(src, docs=True)
    assert routing(with_docs) == {"module:old": {"redirect": "ns.coll.new"}}
    assert with_docs.fragment_routing == {
        "legacy": {"redirect": "ns.coll.current"},
        "gone": {"tombstone": {"removal_version": "2.0.0"}},
    }
    without = run(src)
    assert routing(without) == routing(with_docs)
    assert without.fragment_routing == {}


def test_fragments_name_their_collection():
    files = {
        "MANIFEST.json": manifest({"namespace": "ns", "name": "coll"}),
        "plugins/doc_fragments/auth.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "options: {}"\n',
    }
    assert run(MemSource(files), docs=True).fragments["auth"].collection == "ns.coll"
    del files["MANIFEST.json"]
    assert run(MemSource(files), docs=True).fragments["auth"].collection is None


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        # Tags that aren't YAML's own are ignored and the value read as written.
        # e.g. prometheus.prometheus's process_exporter role argument_specs.
        pytest.param("!unsafe '{{ x }}'", "{{ x }}", id="ansible-unsafe"),
        pytest.param(
            "!vault |\n    $ANSIBLE_VAULT;1.1;AES256\n    6162", "$ANSIBLE_VAULT;1.1;AES256\n6162\n", id="ansible-vault"
        ),
        pytest.param("!custom {a: [1, !other 2]}", {"a": [1, "2"]}, id="custom-mapping"),
        pytest.param("!custom [a, b]", ["a", "b"], id="custom-sequence"),
        # e.g. famedly.matrix: a Matrix room ID as a RETURN sample. Nothing
        # follows the "tag", so it was meant as text.
        pytest.param("!asdfbuiarbk213e479asf:server.tld", "!asdfbuiarbk213e479asf:server.tld", id="text-like-a-tag"),
        pytest.param("!unsafe ''", "", id="tag-with-empty-value"),
        pytest.param("!!python/object/apply:os.system [echo]", ["echo"], id="python-object"),
        pytest.param("!!python/name:os.system", "", id="python-name"),
        # A standard tag whose value doesn't fit it is read as written too.
        pytest.param("!!int abc", "abc", id="bad-int"),
        pytest.param("!!float x", "x", id="bad-float"),
        pytest.param("!!bool maybe", "maybe", id="bad-bool"),
        pytest.param("!!int {a: 1}", {"a": 1}, id="int-on-a-mapping"),
        # Ones that fit are unchanged.
        # A tagged scalar is the text written, as yaml_loads gives it, so these are too.
        pytest.param("!!int 12", "12", id="int"),
        pytest.param("!!null x", "x", id="null"),
        pytest.param("!!str 12", "12", id="str"),
    ],
)
def test_yaml_tags(value: str, expected: object) -> None:
    doc = f"short_description: Kept\nvalue: {value}\n"
    result = run(MemSource({"plugins/modules/m.py": f"DOCUMENTATION = {doc!r}\n"}), docs=True)
    # The rest of the document survives an odd tag.
    assert descriptions(result) == {"m": "Kept"}
    assert plugin_docs(result)["m"].documentation == {"short_description": "Kept", "value": expected}


def test_yaml_tags_in_routing():
    runtime = "plugin_routing:\n  modules:\n    old:\n      deprecation: {removal_version: !!int x, warning_text: !unsafe hi}\n"
    assert routing(run(MemSource({"meta/runtime.yml": runtime}))) == {
        "module:old": {"deprecation": {"removal_version": "x", "warning_text": "hi"}}
    }


# -- Event-Driven Ansible -----------------------------------------------------------


def test_eda_content_in_every_location():
    source_doc = 'DOCUMENTATION = "short_description: {}"\n'
    src = MemSource(
        {
            # Where ansible-rulebook looks, newest layout first.
            "extensions/eda/plugins/event_source/kafka.py": source_doc.format("Kafka"),
            "extensions/eda/plugins/event_sources/webhook.py": source_doc.format("Webhook"),
            "plugins/event_source/legacy.py": source_doc.format("Legacy"),
            "extensions/eda/plugins/event_filter/dashes.py": "",
            "extensions/eda/plugins/event_filters/plural.py": "",
            "plugins/event_filter/old_filter.py": "",
            "extensions/eda/rulebooks/hello.yml": "- name: hello\n",
            "rulebooks/top_level.yml": "",
            # Not loadable, so not listed: a subdirectory (amazon.aws keeps
            # schemas there), a dotted name, __init__, the wrong extension, and
            # a .yaml rulebook (paloaltonetworks.panos has one).
            "extensions/eda/plugins/event_source/schemas/kafka.py": "",
            "extensions/eda/plugins/event_source/my.source.py": "",
            "extensions/eda/plugins/event_source/__init__.py": "",
            "extensions/eda/plugins/event_source/README.md": "",
            "extensions/eda/rulebooks/decryption.yaml": "",
            "extensions/eda/rulebooks/sub/nested.yml": "",
            "extensions/eda/other/thing.py": "",
        }
    )
    result = run(src)
    assert {(p.type, p.name): p.short_description for p in result.plugins} == {
        ("event_filter", "dashes"): None,
        ("event_filter", "old_filter"): None,
        ("event_filter", "plural"): None,
        ("event_source", "kafka"): "Kafka",
        ("event_source", "legacy"): "Legacy",
        ("event_source", "webhook"): "Webhook",
        ("rulebook", "hello"): None,
        ("rulebook", "top_level"): None,
    }


def test_eda_first_location_wins():
    # e.g. chris93111.nats_eda ships the same sources in both layouts.
    # ansible-rulebook loads extensions/eda first, even without docs there.
    src = MemSource(
        {
            "extensions/eda/plugins/event_source/nats.py": "",
            "plugins/event_source/nats.py": 'DOCUMENTATION = "short_description: Old"\n',
            "extensions/eda/rulebooks/site.yml": "",
            "rulebooks/site.yml": "",
        }
    )
    result = run(src, docs=True)
    assert [(p.type, p.name, p.short_description, p.docs) for p in result.plugins] == [
        ("event_source", "nats", None, None),
        ("rulebook", "site", None, None),
    ]


def test_eda_docs_and_unparseable_sources():
    src = MemSource(
        {
            "extensions/eda/plugins/event_source/tick.py": 'DOCUMENTATION = "short_description: Tick"\n'
            + 'EXAMPLES = "- ansible.eda.tick: {}"\n',
            "extensions/eda/plugins/event_filter/broken.py": "print 'python 2'\n",
        }
    )
    result = run(src, docs=True)
    assert {(p.type, p.name): p.docs and without_path(p.docs) for p in result.plugins} == {
        # Python 2, so an error of the whole file.
        ("event_filter", "broken"): ext.PluginDocs(),
        ("event_source", "tick"): ext.PluginDocs(
            documentation={"short_description": "Tick"}, examples="- ansible.eda.tick: {}"
        ),
    }
    assert issues(result, "python_unparseable") == ["extensions/eda/plugins/event_filter/broken.py"]


def test_eda_routing_comes_from_eda_runtime():
    # e.g. ansible.eda, which deprecates sources that moved to other collections.
    eda_runtime = """
plugin_routing:
  event_source:
    aws_sqs_queue:
      deprecation: {removal_version: "3.0.0", warning_text: Please use amazon.aws.aws_sqs_queue instead.}
    moved: {redirect: ns.other.moved}
  event_filter:
    noop:
      tombstone: {removal_version: "3.0.0", warning_text: Stop using it.}
  modules:
    not_eda: {redirect: ns.coll.other}
"""
    # ansible-rulebook doesn't read EDA routing from meta/runtime.yml.
    runtime = "plugin_routing:\n  event_source:\n    ignored: {redirect: ns.coll.x}\n"
    result = run(MemSource({"extensions/eda/eda_runtime.yml": eda_runtime, "meta/runtime.yml": runtime}))
    assert routing(result) == {
        "event_filter:noop": {"tombstone": {"removal_version": "3.0.0", "warning_text": "Stop using it."}},
        "event_source:aws_sqs_queue": {
            "deprecation": {"removal_version": "3.0.0", "warning_text": "Please use amazon.aws.aws_sqs_queue instead."}
        },
        "event_source:moved": {"redirect": "ns.other.moved"},
    }
