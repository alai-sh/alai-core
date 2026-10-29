# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Applying doc fragments as ansible-core does, ported with alai-plugin-extractor's tests."""

from __future__ import annotations

import collections.abc as _c
import copy
import json
import pathlib
import tempfile
import types
import typing as _t

import pytest

from alai_core import extract as ext

Fragments = dict[str, dict[str, dict[str, object]]]
"""Fragments by full name, each with its attributes."""


def labelled(docs: ext.PluginDocs, collection: str, module: bool = False) -> ext.PluginDocs:
    """``docs`` as a plugin of ``collection``'s has them, to label its versions with."""
    return ext.PluginDocs(
        documentation=docs.documentation,
        examples=docs.examples,
        returns=docs.returns,
        entry_points=docs.entry_points,
        fragments=docs.fragments,
        return_fragments=docs.return_fragments,
        collection=collection,
        module=module,
    )


class Lookup:
    """A fragment lookup over {full name: attributes} that records the names it was asked for."""

    def __init__(self, fragments: _c.Mapping[str, _c.Mapping[str, _c.Mapping[str, object]]]) -> None:
        self.fragments = fragments
        self.calls: list[str] = []

    def __call__(self, name: str) -> ext.DocFragment | None:
        self.calls.append(name)
        if (attributes := self.fragments.get(name)) is None:
            return None
        namespace, collection, short = name.split(".", 2)
        return ext.DocFragment(collection=f"{namespace}.{collection}", name=short, attributes=attributes)


def test_merge_follows_ansible_core():
    docs = ext.PluginDocs(
        documentation={
            "short_description": "Thing",
            "author": "Plugin Author",
            "notes": ["Plugin note."],
            "requirements": ["b", "a"],
            "options": {
                "host": {"type": "str", "description": "Plugin's host."},
                "own": {"type": "int"},
            },
            "attributes": {"check_mode": {"support": "full"}},
        },
        fragments=("ns.other.auth",),
    )
    auth = {
        "DOCUMENTATION": {
            "author": "Fragment Author",
            "notes": ["Fragment note."],
            "seealso": [{"module": "ns.other.login"}],
            "requirements": ["a", "c"],
            "version_added": "1.0.0",
            "options": {
                "host": {"type": "str", "description": "Fragment's host.", "required": True},
                "token": {"type": "str", "no_log": True},
            },
            "attributes": {"check_mode": {"support": "none"}, "diff_mode": {"support": "none"}},
        }
    }
    merged = docs.apply_fragments(Lookup({"ns.other.auth": auth}))
    assert merged == ext.PluginDocs(
        documentation={
            "short_description": "Thing",
            # The plugin's own scalars win.
            "author": "Plugin Author",
            # notes and seealso are extended.
            "notes": ["Plugin note.", "Fragment note."],
            "seealso": [{"module": "ns.other.login"}],
            # Other lists are combined.
            "requirements": ["a", "b", "c"],
            # Keys only the fragment has are added.
            "version_added": "1.0.0",
            # Options and attributes merge per entry, the plugin's keys winning.
            "options": {
                "host": {"type": "str", "description": "Plugin's host.", "required": True},
                "own": {"type": "int"},
                "token": {"type": "str", "no_log": True},
            },
            "attributes": {"check_mode": {"support": "full"}, "diff_mode": {"support": "none"}},
        },
    )


def test_any_mapping_given_as_a_dict() -> None:
    # Copied when made, so a mapping of any type, at any level, comes back as a dict, and changing it later doesn't
    # change the docs.
    options = {"a": {"type": "str"}}
    docs = ext.PluginDocs(documentation=types.MappingProxyType({"options": types.MappingProxyType(options)}))
    fragment = ext.DocFragment(
        collection=None, name="f", attributes=types.MappingProxyType({"DOCUMENTATION": types.MappingProxyType({})})
    )
    options["b"] = {"type": "int"}

    assert docs.documentation == {"options": {"a": {"type": "str"}}}
    assert type(docs.documentation) is dict and type(docs.documentation["options"]) is dict
    assert fragment.attributes == {"DOCUMENTATION": {}}


def test_not_a_value() -> None:
    with pytest.raises(TypeError, match="can't use object in docs"):
        _ = ext.PluginDocs(documentation={"options": object()})
    with pytest.raises(TypeError, match="documentation must be a mapping or None"):
        _ = ext.PluginDocs(documentation=_t.cast(_t.Any, ["a"]))


def test_inputs_are_not_modified():
    docs = ext.PluginDocs(documentation={"options": {"a": {"type": "str"}}}, fragments=("ns.c.f",))
    fragments = {"ns.c.f": {"DOCUMENTATION": {"options": {"b": {"type": "int"}}, "notes": ["n"]}}}
    before = copy.deepcopy((docs, fragments))

    merged = docs.apply_fragments(Lookup(fragments))
    documentation = _t.cast(dict[str, _t.Any], merged.documentation)
    documentation["options"]["b"]["type"] = "changed"
    documentation["notes"].append("changed")
    assert (docs, fragments) == before


def test_names_resolve_like_ansible_core():
    docs = ext.PluginDocs(
        documentation={},
        fragments=(
            "files",  # a short name is ansible.builtin's
            "action_common_attributes.conn",  # .conn is the CONN attribute
            "ns.coll.frag",
            "ns.coll.frag.facts",
            "ns.coll.sub.name",  # a fragment in a subdirectory, not an attribute
        ),
    )
    lookup = Lookup(
        {
            "ansible.builtin.files": {"DOCUMENTATION": {"options": {"mode": {}}}},
            "ansible.builtin.action_common_attributes": {"CONN": {"attributes": {"connection": {}}}},
            "ns.coll.frag": {"DOCUMENTATION": {"options": {"a": {}}}, "FACTS": {"options": {"facts": {}}}},
            "ns.coll.sub.name": {"DOCUMENTATION": {"options": {"sub": {}}}},
        }
    )
    merged = docs.apply_fragments(lookup)
    assert merged.documentation == {
        "options": {"mode": {}, "a": {}, "facts": {}, "sub": {}},
        "attributes": {"connection": {}},
    }
    assert merged.fragments == ()
    assert lookup.calls == [
        "ansible.builtin.files",
        "ansible.builtin.action_common_attributes.conn",
        "ansible.builtin.action_common_attributes",
        "ns.coll.frag",
        "ns.coll.frag.facts",
        "ns.coll.frag",
        "ns.coll.sub.name",
    ]


def test_unresolved_names_stay_for_a_later_call():
    docs = ext.PluginDocs(
        documentation={"options": {"own": {}}},
        fragments=("ns.one.frag", "ns.two.frag", "ns.one.frag.missing_attribute", "nodots"),
    )
    one: Fragments = {"ns.one.frag": {"DOCUMENTATION": {"options": {"one": {}}}}}
    first = docs.apply_fragments(Lookup(one))
    assert first.fragments == ("ns.two.frag", "ns.one.frag.missing_attribute", "nodots")
    assert first.documentation == {"options": {"own": {}, "one": {}}}

    two: Fragments = {"ns.two.frag": {"DOCUMENTATION": {"options": {"two": {}}}}}
    second = first.apply_fragments(Lookup(two))
    assert second.fragments == ("ns.one.frag.missing_attribute", "nodots")
    assert second.documentation == {"options": {"own": {}, "one": {}, "two": {}}}


def test_return_fragments():
    docs = ext.PluginDocs(
        returns={"changed": {"type": "bool"}},
        return_fragments=("ns.c.results", "ns.c.no_return"),
    )
    fragments: Fragments = {
        "ns.c.results": {"RETURN": {"stdout": {"type": "str"}}, "DOCUMENTATION": {"options": {"x": {}}}},
        # A fragment without RETURN adds nothing, as in ansible-core.
        "ns.c.no_return": {"DOCUMENTATION": {"options": {"y": {}}}},
    }
    merged = docs.apply_fragments(Lookup(fragments))
    assert merged == ext.PluginDocs(returns={"changed": {"type": "bool"}, "stdout": {"type": "str"}})


def test_nothing_to_apply_returns_the_same_docs():
    docs = ext.PluginDocs(documentation={"a": 1}, examples="- x: 1")
    lookup = Lookup({})
    assert docs.apply_fragments(lookup) == docs
    assert lookup.calls == []


def test_conflicts_ansible_core_rejects_keep_the_plugin_value():
    docs = ext.PluginDocs(
        documentation={
            "options": None,  # not a dict, so the fragment's options are dropped
            "notes": "a string",  # replaced by the fragment's list
            "seealso": [{"ref": "a"}],
            "items": [{"a": 1}, 2],  # dicts can't be sorted into a set
            "mixed": [1],
        },
        fragments=("ns.c.f",),
    )
    fragment: dict[str, dict[str, object]] = {
        "DOCUMENTATION": {
            "options": {"x": {}},
            "notes": ["Fragment note."],
            "seealso": "not a list",  # ignored, as ansible-core can't extend with it either
            "items": [{"a": 1}, {"b": 2}],
            "mixed": ["x"],  # ints and strings can't be sorted together
        }
    }
    merged = docs.apply_fragments(Lookup({"ns.c.f": fragment}))
    assert merged.documentation == {
        "options": None,
        "notes": ["Fragment note."],
        "seealso": [{"ref": "a"}],
        "items": [{"a": 1}, 2, {"b": 2}],
        "mixed": [1, "x"],
    }


def test_fragments_for_docs_without_documentation():
    # Only possible for PluginDocs built by the caller; the fragment becomes the docs.
    docs = ext.PluginDocs(fragments=("ns.c.f",))
    merged = docs.apply_fragments(Lookup({"ns.c.f": {"DOCUMENTATION": {"options": {"x": {}}}}}))
    assert merged.documentation == {"options": {"x": {}}}


def manifest(namespace: str, name: str) -> str:
    """A MANIFEST.json as ansible-galaxy builds it."""
    files = {"name": "FILES.json", "ftype": "file", "chksum_type": "sha256", "chksum_sha256": "0" * 64, "format": 1}
    info = {"namespace": namespace, "name": name, "version": "1.0.0"}
    return json.dumps({"collection_info": info, "file_manifest_file": files, "format": 1})


def test_with_extracted_collections(tmp_path: pathlib.Path) -> None:
    def write(root: pathlib.Path, files: dict[str, str]) -> None:
        for name, content in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            _ = (root / name).write_text(content)

    write(
        tmp_path / "other",
        {
            "MANIFEST.json": manifest("ns", "other"),
            "plugins/doc_fragments/auth.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "options: {token: {}}"\n',
        },
    )
    write(
        tmp_path / "coll",
        {
            "MANIFEST.json": manifest("ns", "coll"),
            "plugins/modules/m.py": 'DOCUMENTATION = """\nshort_description: M\nextends_documentation_fragment:\n'
            + '- ns.other.auth\n- ns.coll.common\n"""\n',
            "plugins/doc_fragments/common.py": 'class ModuleDocFragment:\n    DOCUMENTATION = "options: {x: {}}"\n',
        },
    )
    coll = ext.extract(tmp_path / "coll", docs=True)
    other = ext.extract(tmp_path / "other", docs=True)
    docs = coll.plugins[0].docs
    assert docs is not None
    merged = docs.apply_fragments(ext.fragment_lookup([coll, other]))
    assert merged == ext.PluginDocs(
        documentation={"short_description": "M", "options": {"token": {}, "x": {}}},
        path="plugins/modules/m.py",
        # From extraction, which knows both, so apply_fragments needs neither.
        collection="ns.coll",
        module=True,
    )


# -- fragment_lookup ------------------------------------------------------------


def collection(
    namespace: str | None,
    name: str | None,
    fragments: _c.Mapping[str, object],
    routing: _c.Mapping[str, object] | None = None,
) -> ext.Collection:
    """A collection with ``fragments``, each a name and its DOCUMENTATION, and their ``routing``, extracted."""
    with tempfile.TemporaryDirectory() as root:
        files = {
            f"plugins/doc_fragments/{n.replace('.', '/')}.py": "class ModuleDocFragment:\n"
            + f"    DOCUMENTATION = {json.dumps(doc)!r}\n"
            for n, doc in fragments.items()
        }
        if namespace is not None and name is not None:
            files["MANIFEST.json"] = manifest(namespace, name)
        if routing:
            files["meta/runtime.yml"] = json.dumps({"plugin_routing": {"doc_fragments": routing}})
        for path, content in files.items():
            pathlib.Path(root, path).parent.mkdir(parents=True, exist_ok=True)
            _ = pathlib.Path(root, path).write_text(content)
        return ext.extract(root, docs=True)


def test_fragment_lookup_follows_routing():
    builtin = collection(
        "ansible",
        "builtin",
        {"files": {"options": {"mode": {}}}},
        {
            "azure": {"redirect": "azure.azcollection.azure"},
            "old": {"redirect": "ns.coll.moved"},
            "gone": {"tombstone": {"removal_version": "2.0.0"}},
            "loop_a": {"redirect": "ansible.builtin.loop_b"},
            "loop_b": {"redirect": "loop_a"},  # short names are ansible.builtin's here too
            "bad": {"redirect": ["not", "a", "name"]},
        },
    )
    azure = collection("azure", "azcollection", {"azure": {"options": {"auth": {}}}})
    coll = collection(
        "ns",
        "coll",
        {"real": {"options": {"real": {}}}, "files": {"options": {"shadowed": {}}}},
        {
            "moved": {"redirect": "ns.coll.real"},  # a chain: ansible.builtin.old -> ns.coll.moved -> ns.coll.real
            "files": {"redirect": "ansible.builtin.files"},  # routing wins over the file
            "warned": {"deprecation": {"removal_version": "3.0.0"}, "redirect": "ns.coll.real"},
        },
    )
    nameless = collection(None, None, {"orphan": {"options": {}}})
    lookup = ext.fragment_lookup([builtin, azure, coll, nameless])

    def options(name: str) -> tuple[str | None, list[str]] | None:
        fragment = lookup(name)
        if fragment is None:
            return None
        options = _t.cast(dict[str, object], fragment.attributes["DOCUMENTATION"]["options"])
        return (fragment.collection, list(options))

    assert options("ansible.builtin.azure") == ("azure.azcollection", ["auth"])
    assert options("ansible.builtin.old") == ("ns.coll", ["real"])
    assert options("ns.coll.files") == ("ansible.builtin", ["mode"])
    assert options("ns.coll.warned") == ("ns.coll", ["real"])
    assert options("ansible.builtin.gone") is None
    assert options("ansible.builtin.loop_a") is None
    assert options("ansible.builtin.bad") is None
    assert options("ns.coll.missing") is None


def test_redirected_short_names_apply():
    # e.g. azure, which ansible.builtin redirects to azure.azcollection.azure.
    builtin = collection("ansible", "builtin", {}, {"azure": {"redirect": "azure.azcollection.azure"}})
    azure = collection("azure", "azcollection", {"azure": {"options": {"auth": {"version_added": "1.0.0"}}}})
    docs = ext.PluginDocs(documentation={"options": {}}, fragments=("azure",))
    merged = labelled(docs, "ns.coll", module=True).apply_fragments(ext.fragment_lookup([builtin, azure]))
    # The label is the collection the fragment really came from.
    assert merged.documentation == {
        "options": {"auth": {"version_added": "1.0.0", "version_added_collection": "azure.azcollection"}}
    }


# -- labelling versions with their collection -------------------------------------


def test_versions_are_labelled_for_a_plugin():
    docs = ext.PluginDocs(
        documentation={
            "version_added": "1.0.0",
            "deprecated": {"removed_at_date": "2030-01-01", "why": "old"},
            "options": {
                "a": {
                    "version_added": "1.1.0",
                    "env": [
                        {"name": "A", "version_added": "1.2.0", "deprecated": {"version": "3.0.0"}},
                        {"name": "B"},
                        "junk",
                    ],
                    "ini": "not a list",
                    "deprecated": {"version": "4.0.0"},
                    "suboptions": {"b": {"version_added": "1.3.0"}, "c": "junk"},
                },
                "untouched": {"type": "str"},
                "labelled": {"version_added": "0.1.0", "version_added_collection": "ns.elsewhere"},
                "junk": "not a dict",
            },
            "attributes": {"check_mode": {"version_added": "1.4.0"}, "diff_mode": {"support": "full"}},
        },
        returns={
            "out": {"version_added": "1.5.0", "contains": {"inner": {"version_added": "1.6.0"}}},
            "plain": {"type": "str"},
        },
    )
    result = labelled(docs, "ns.coll").apply_fragments(Lookup({}))
    assert result.documentation == {
        "version_added": "1.0.0",
        "version_added_collection": "ns.coll",
        "deprecated": {"removed_at_date": "2030-01-01", "why": "old", "removed_from_collection": "ns.coll"},
        "options": {
            "a": {
                "version_added": "1.1.0",
                "version_added_collection": "ns.coll",
                "env": [
                    {
                        "name": "A",
                        "version_added": "1.2.0",
                        "version_added_collection": "ns.coll",
                        "deprecated": {"version": "3.0.0", "collection_name": "ns.coll"},
                    },
                    {"name": "B"},
                    "junk",
                ],
                "ini": "not a list",
                "deprecated": {"version": "4.0.0", "collection_name": "ns.coll"},
                "suboptions": {"b": {"version_added": "1.3.0", "version_added_collection": "ns.coll"}, "c": "junk"},
            },
            "untouched": {"type": "str"},
            # An existing label is kept.
            "labelled": {"version_added": "0.1.0", "version_added_collection": "ns.elsewhere"},
            "junk": "not a dict",
        },
        "attributes": {
            "check_mode": {"version_added": "1.4.0", "version_added_collection": "ns.coll"},
            "diff_mode": {"support": "full"},
        },
    }
    assert result.returns == {
        "out": {
            "version_added": "1.5.0",
            "version_added_collection": "ns.coll",
            "contains": {"inner": {"version_added": "1.6.0", "version_added_collection": "ns.coll"}},
        },
        "plain": {"type": "str"},
    }
    # Unlabelled without a collection.
    assert docs.apply_fragments(Lookup({})) == docs


def test_module_deprecations_have_a_different_shape():
    doc = {
        "deprecated": {"removed_in": "3.0.0"},
        "options": {
            "a": {
                "env": [{"version_added": "1.0.0"}],  # modules don't read env, so it isn't labelled
                "deprecated": {"version": "3.0.0"},  # nor option deprecations
            }
        },
    }
    module = labelled(ext.PluginDocs(documentation=doc), "ns.coll", module=True).apply_fragments(Lookup({}))
    assert module.documentation == {
        "deprecated": {"removed_in": "3.0.0", "removed_from_collection": "ns.coll"},
        "options": {"a": {"env": [{"version_added": "1.0.0"}], "deprecated": {"version": "3.0.0"}}},
    }
    plugin = labelled(
        ext.PluginDocs(
            documentation={
                "deprecated": {"removed_in": "3.0.0"},
                "options": {"a": {"deprecated": {"removed_in": "3.0.0"}}},
            }
        ),
        "ns.coll",
    ).apply_fragments(Lookup({}))
    # A plugin's top-level deprecation is labelled from removed_in, an option's isn't.
    assert plugin.documentation == {
        "deprecated": {"removed_in": "3.0.0", "removed_from_collection": "ns.coll"},
        "options": {"a": {"deprecated": {"removed_in": "3.0.0"}}},
    }


def test_fragment_versions_are_labelled_with_their_collection():
    docs = ext.PluginDocs(
        documentation={"options": {"both": {"version_added": "2.0.0"}, "own": {"version_added": "2.1.0"}}},
        returns={"rc": {"version_added": "2.2.0"}},
        fragments=("ns.other.frag", "ns.nameless.frag"),
        return_fragments=("ns.other.frag",),
    )
    other = ext.DocFragment(
        collection="ns.other",
        name="frag",
        attributes={
            "DOCUMENTATION": {"options": {"both": {"version_added": "1.0.0"}, "theirs": {"version_added": "1.1.0"}}},
            "RETURN": {"stdout": {"version_added": "1.2.0"}},
        },
    )
    nameless = ext.DocFragment(
        collection=None, name="frag", attributes={"DOCUMENTATION": {"options": {"anon": {"version_added": "1.3.0"}}}}
    )
    fragments = {"ns.other.frag": other, "ns.nameless.frag": nameless}
    merged = labelled(docs, "ns.coll").apply_fragments(fragments.get)
    assert merged.documentation == {
        "options": {
            # Labelled before merging, so the plugin's version keeps its own collection.
            "both": {"version_added": "2.0.0", "version_added_collection": "ns.coll"},
            "own": {"version_added": "2.1.0", "version_added_collection": "ns.coll"},
            "theirs": {"version_added": "1.1.0", "version_added_collection": "ns.other"},
            # A fragment from a collection with no known name isn't labelled.
            "anon": {"version_added": "1.3.0"},
        }
    }
    assert merged.returns == {
        "rc": {"version_added": "2.2.0", "version_added_collection": "ns.coll"},
        "stdout": {"version_added": "1.2.0", "version_added_collection": "ns.other"},
    }


def test_labelling_docs_without_returns_leaves_them_none():
    docs = ext.PluginDocs(examples="- x: 1")
    assert labelled(docs, "ns.coll").apply_fragments(Lookup({})) == labelled(docs, "ns.coll")


def test_empty_notes_in_a_fragment_add_nothing():
    # e.g. community.general's _hpe3par, whose DOCUMENTATION has `notes:` with no value.
    docs = ext.PluginDocs(documentation={"options": {}}, fragments=("ns.c.f",))
    fragment: dict[str, dict[str, object]] = {"DOCUMENTATION": {"options": {"x": {}}, "notes": None, "seealso": []}}
    assert docs.apply_fragments(Lookup({"ns.c.f": fragment})).documentation == {"options": {"x": {}}}
