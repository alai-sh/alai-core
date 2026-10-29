# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Extraction of ansible-core's own plugins, as ansible.builtin, and ansible-rulebook's, as eda.builtin, from their
source laid out as each ships it."""

from __future__ import annotations

import collections.abc as _c
from pathlib import Path

import pytest

from alai_core import extract as ext

RELEASE = "__version__ = '2.21.5'\n"


def write(root: Path, files: _c.Mapping[str, str]) -> Path:
    for name, content in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        _ = path.write_text(content)
    return root


def doc(short: str) -> str:
    return f'DOCUMENTATION = """\nshort_description: {short}\n"""\n'


def found(result: ext.Collection) -> dict[str, str | None]:
    return {f"{p.type}:{p.name}": p.short_description for p in result.plugins}


def test_ansible_core_layout(tmp_path: Path) -> None:
    root = write(
        tmp_path,
        {
            "lib/ansible/release.py": RELEASE,
            "lib/ansible/modules/__init__.py": "",
            "lib/ansible/modules/copy.py": doc("Copy files"),
            "lib/ansible/plugins/__init__.py": "",
            "lib/ansible/plugins/loader.py": "",
            "lib/ansible/plugins/action/copy.py": "",
            "lib/ansible/plugins/action/debug.py": doc("Print"),
            "lib/ansible/plugins/lookup/__init__.py": "",
            "lib/ansible/plugins/lookup/file.py": doc("Read a file"),
            "lib/ansible/plugins/filter/core.py": (
                "class FilterModule:\n    def filters(self):\n        return {'to_json': a}\n"
            ),
            "lib/ansible/plugins/filter/to_json.yml": "DOCUMENTATION:\n  short_description: To JSON\n",
            "lib/ansible/config/ansible_builtin_runtime.yml": (
                "plugin_routing:\n  modules:\n    docker_container:\n      redirect: community.docker.docker_container\n"
            ),
            # Only what the layout names is read.
            "lib/ansible/cli/doc.py": doc("Not a plugin"),
            "plugins/modules/stray.py": doc("Not ansible-core's"),
            "meta/runtime.yml": "plugin_routing:\n  modules:\n    stray:\n      redirect: a.b.c\n",
            "MANIFEST.json": "not read",
        },
    )

    result = ext.ansible_core(root)

    assert (result.namespace, result.name, result.version) == ("ansible", "builtin", "2.21.5")
    assert (result.dependencies, result.requires_ansible, result.issues) == ({}, None, ())
    assert found(result) == {
        "filter:to_json": "To JSON",
        "lookup:file": "Read a file",
        "module:copy": "Copy files",
        "module:debug": "Print",
    }
    assert [(r.type, r.name, r.entry) for r in result.routing] == [
        ("module", "docker_container", {"redirect": "community.docker.docker_container"}),
    ]


def test_ansible_core_modules_come_from_lib_ansible_modules(tmp_path: Path) -> None:
    # lib/ansible/plugins/modules/ would be plugins/modules/, which the layout reads from lib/ansible/modules/.
    root = write(
        tmp_path,
        {
            "lib/ansible/release.py": RELEASE,
            "lib/ansible/modules/ping.py": doc("Ping"),
            "lib/ansible/plugins/modules/ping.py": doc("Shadowed"),
            "lib/ansible/plugins/modules/other.py": doc("Shadowed"),
        },
    )

    assert found(ext.ansible_core(root)) == {"module:ping": "Ping"}


def test_ansible_core_version(tmp_path: Path) -> None:
    root = write(tmp_path, {"lib/ansible/release.py": "__version__ = get_version()\n"})

    assert ext.ansible_core(root).version is None


def test_not_ansible_core(tmp_path: Path) -> None:
    _ = write(tmp_path, {"lib/ansible/modules/ping.py": doc("Ping")})

    with pytest.raises(FileNotFoundError, match=r"isn't ansible-core's source: it has no lib/ansible/release\.py"):
        _ = ext.ansible_core(tmp_path)


def test_not_a_directory(tmp_path: Path) -> None:
    path = tmp_path / "file"
    _ = path.write_text("")

    with pytest.raises(NotADirectoryError):
        _ = ext.ansible_core(path)
    with pytest.raises(NotADirectoryError):
        _ = ext.ansible_rulebook(path)


def test_ansible_builtin_fragments_for_a_collection(tmp_path: Path) -> None:
    core = write(
        tmp_path / "core",
        {
            "lib/ansible/release.py": RELEASE,
            "lib/ansible/plugins/doc_fragments/files.py": (
                'class ModuleDocFragment:\n    DOCUMENTATION = """\noptions:\n  mode:\n    type: raw\n"""\n'
            ),
            "lib/ansible/config/ansible_builtin_runtime.yml": (
                "plugin_routing:\n  doc_fragments:\n    aws:\n      redirect: amazon.aws.common\n"
            ),
        },
    )
    aws = write(
        tmp_path / "aws",
        {
            "galaxy.yml": "namespace: amazon\nname: aws\nversion: 1.0.0\n",
            "plugins/doc_fragments/common.py": (
                'class ModuleDocFragment:\n    DOCUMENTATION = """\noptions:\n  region:\n    type: str\n"""\n'
            ),
        },
    )
    coll = write(
        tmp_path / "coll",
        {
            "galaxy.yml": "namespace: ns\nname: coll\nversion: 1.0.0\n",
            "plugins/modules/thing.py": (
                'DOCUMENTATION = """\nshort_description: Thing\nextends_documentation_fragment: [files, aws]\n"""\n'
            ),
        },
    )
    builtin = ext.ansible_core(core, docs=True)
    thing = ext.extract(coll, docs=True).plugins[0]
    assert thing.docs is not None

    merged = thing.docs.apply_fragments(ext.fragment_lookup([builtin, ext.extract(aws, docs=True)]))

    assert (builtin.namespace, builtin.name) == ("ansible", "builtin")
    assert list(builtin.fragments) == ["files"]
    assert builtin.fragment_routing == {"aws": {"redirect": "amazon.aws.common"}}
    assert merged.fragments == ()
    assert merged.documentation is not None and merged.documentation["options"] == {
        "mode": {"type": "raw"},
        "region": {"type": "str"},
    }


def test_ansible_rulebook_layout(tmp_path: Path) -> None:
    root = write(
        tmp_path,
        {
            "PKG-INFO": "Metadata-Version: 2.4\nName: ansible_rulebook\nVersion: 1.3.2\n\nVersion: 9.9.9\n",
            "ansible_rulebook/__init__.py": "",
            "ansible_rulebook/event_source/__init__.py": "",
            "ansible_rulebook/event_source/range.py": doc("Generate events"),
            "ansible_rulebook/event_filter/noop.py": doc("Do nothing"),
            "ansible_rulebook/event_filter/insert_meta_info.py": '"""No DOCUMENTATION."""\n',
            # Only the two directories ansible-rulebook loads eda.builtin from are read.
            "ansible_rulebook/action/noop.py": doc("An action"),
            "extensions/eda/plugins/event_source/stray.py": doc("Not ansible-rulebook's"),
        },
    )

    result = ext.ansible_rulebook(root)

    assert (result.namespace, result.name, result.version) == ("eda", "builtin", "1.3.2")
    assert (result.dependencies, result.requires_ansible, result.routing, result.issues) == ({}, None, (), ())
    assert found(result) == {
        "event_filter:insert_meta_info": None,
        "event_filter:noop": "Do nothing",
        "event_source:range": "Generate events",
    }


def test_ansible_rulebook_checkout_has_no_version(tmp_path: Path) -> None:
    root = write(tmp_path, {"ansible_rulebook/__init__.py": ""})

    assert ext.ansible_rulebook(root).version is None


def test_not_ansible_rulebook(tmp_path: Path) -> None:
    _ = write(tmp_path, {"ansible_rulebook/event_source/range.py": doc("Generate events")})

    with pytest.raises(
        FileNotFoundError, match=r"isn't ansible-rulebook's source: it has no ansible_rulebook/__init__\.py"
    ):
        _ = ext.ansible_rulebook(tmp_path)


def test_eda_legacy_routing() -> None:
    assert ext.EDA_LEGACY_ROUTING == {
        "event_source": {
            "ansible.eda.generic": "eda.builtin.generic",
            "ansible.eda.pg_listener": "eda.builtin.pg_listener",
            "ansible.eda.range": "eda.builtin.range",
        },
        "event_filter": {
            "ansible.eda.dashes_to_underscores": "eda.builtin.dashes_to_underscores",
            "ansible.eda.insert_hosts_to_meta": "eda.builtin.insert_hosts_to_meta",
            "ansible.eda.json_filter": "eda.builtin.json_filter",
            "ansible.eda.noop": "eda.builtin.noop",
            "ansible.eda.normalize_keys": "eda.builtin.normalize_keys",
        },
    }
