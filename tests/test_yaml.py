# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import warnings

import pytest

from alai_core.yaml import YamlError, YamlWarning, loads, select


class TestScalars:
    @pytest.mark.parametrize(
        ("text", "value"),
        [
            ("yes", True),
            ("No", False),
            ("ON", True),
            ("off", False),
            ("true", True),
            ("y", "y"),
            ("~", None),
            ("null", None),
            ("", None),
            ("42", 42),
            ("-7", -7),
            ("99999999999999999999", 99999999999999999999),
            ("2.9", 2.9),
            ("1.0", 1.0),
            ("2.10", "2.10"),
            ("0644", "0644"),
            ("0x1F", "0x1F"),
            ("1_000", "1_000"),
            ("1e3", "1e3"),
            ("11:33:47", "11:33:47"),
            (".inf", ".inf"),
            ("2021-01-01", "2021-01-01"),
            ("1.0.0", "1.0.0"),
        ],
    )
    def test_plain(self, text: str, value: object) -> None:
        got = loads(f"k: {text}\n")

        assert got == {"k": value}
        assert type(got["k"]) is type(value)  # pyright: ignore[reportIndexIssue]

    @pytest.mark.parametrize("text", ['"yes"', "'2.9'", "!!int 1", "!unsafe yes", "|\n  1"])
    def test_quoted_block_or_tagged_is_a_string(self, text: str) -> None:
        assert type(loads(f"k: {text}\n")["k"]) is str  # pyright: ignore[reportIndexIssue]

    def test_tag_alone_is_its_text(self) -> None:
        assert loads("sample: !abc:server.tld\n") == {"sample": "!abc:server.tld"}

    def test_keys_resolve_too(self) -> None:
        assert loads("1: a\nno: b\nnull: c\n2.9: d\n") == {1: "a", False: "b", None: "c", 2.9: "d"}


class TestDocuments:
    def test_empty(self) -> None:
        assert loads("") is None
        assert loads("# only a comment\n") is None

    def test_anchor_and_alias_are_the_same_object(self) -> None:
        doc = loads("a: &x {b: [1]}\nc: *x\n")

        assert doc == {"a": {"b": [1]}, "c": {"b": [1]}}
        assert doc["a"] is doc["c"]  # pyright: ignore[reportIndexIssue]

    def test_merge_keys(self) -> None:
        doc = loads("p: &p {p: 1, x: 1}\nq: &q {q: 2, x: 2}\nr: {<<: [*p, *q], o: 3}\ns: {'<<': 1}\n")

        assert list(doc["r"].items()) == [("q", 2), ("x", 1), ("p", 1), ("o", 3)]  # pyright: ignore[reportIndexIssue]
        assert doc["s"] == {"<<": 1}  # pyright: ignore[reportIndexIssue]

    def test_block_scalar_at_the_end_without_a_newline(self) -> None:
        assert loads("a: |\n  x\n  y") == {"a": "x\ny"}

    @pytest.mark.parametrize(
        ("text", "message"),
        [
            ("a: [", r"did not find expected node content .* \(at line 2, column 1\)"),
            ("a: 1\n---\nb: 2\n", "expected a single document"),
            ("? [1]\n: x\n", "found unhashable key"),
            ("a: *nope\n", "found undefined alias 'nope'"),
            ("[" * 257 + "]" * 257, "nests deeper than 256"),
        ],
    )
    def test_refused(self, text: str, message: str) -> None:
        with pytest.raises(YamlError, match=message):
            _ = loads(text)

    def test_named(self) -> None:
        with pytest.raises(YamlError, match=r"^plugins/modules/foo\.py: did not find expected node content") as e:
            _ = loads("a: [", name="plugins/modules/foo.py")

        assert e.value.name == "plugins/modules/foo.py"

    def test_error_position(self) -> None:
        with pytest.raises(YamlError) as e:
            _ = loads("a: [")

        assert (e.value.name, e.value.line, e.value.column) == (None, 2, 1)
        assert isinstance(e.value, ValueError)


class TestWhatAnsibleReads:
    """Text Ansible reads, as collections write it. Some is YAML a stricter parser refuses, which is why libyaml's
    parser is used."""

    @pytest.mark.parametrize(
        ("text", "value"),
        [
            pytest.param(
                'rules:\n    sample: {\n        "key": {\n            "foo": "write"\n        }\n    }\nnext: 1\n',
                {"rules": {"sample": {"key": {"foo": "write"}}}, "next": 1},
                id="flow-mapping-closed-at-its-parents-indentation",
            ),
            pytest.param(
                'counters:\n    description:\n    - "all = all; req = Request; req-reject = Request\n      Rejected"\n',
                {"counters": {"description": ["all = all; req = Request; req-reject = Request Rejected"]}},
                id="quoted-string-continued-under-indented",
            ),
            pytest.param(
                "opt:\n    choices: [ a, b,\n    c, d ]\n",
                {"opt": {"choices": ["a", "b", "c", "d"]}},
                id="flow-sequence-wrapped-at-its-parents-indentation",
            ),
            pytest.param(
                'sample: \'{\n    "a": {\n\t\t"b": 1}}\'\n',
                {"sample": '{ "a": { "b": 1}}'},
                id="tab-in-a-quoted-string",
            ),
            pytest.param("---\nmodule: foo\n", {"module": "foo"}, id="document-start"),
            pytest.param("module: foo\n...\n", {"module": "foo"}, id="document-end"),
            pytest.param("%YAML 1.1\n---\na: yes\n", {"a": True}, id="yaml-1.1-directive"),
            pytest.param("%YAML 1.2\n---\na: yes\n", {"a": True}, id="yaml-1.2-directive"),
            pytest.param("description: Ünïcödé ☃\n键: 值\n", {"description": "Ünïcödé ☃", "键": "值"}, id="non-ascii"),
            pytest.param("a: 1\r\nb:\r\n  - x\r\n  - y\r\n", {"a": 1, "b": ["x", "y"]}, id="crlf"),
            pytest.param("\ufeffa: 1\n", {"a": 1}, id="byte-order-mark"),
            pytest.param(
                "sample:\n  200: OK\n  404: Not Found\n  name: x\n",
                {"sample": {200: "OK", 404: "Not Found", "name": "x"}},
                id="integer-and-string-keys",
            ),
        ],
    )
    def test_as_pyyaml_reads_it(self, text: str, value: object) -> None:
        assert loads(text) == value

    def test_not_unicode(self) -> None:
        # A lone surrogate, as decoding with surrogateescape leaves for a byte that isn't UTF-8.
        with pytest.raises(YamlError, match=r"^foo\.yml: the text isn't valid Unicode: .*surrogates not allowed") as e:
            _ = loads("a: \udc80\n", name="foo.yml")

        assert (e.value.line, e.value.column) == (None, None)


class TestWarnings:
    TEXT = "a: 1\nb: 2\na: 3\n"

    def test_python_warning_by_default(self) -> None:
        with pytest.warns(YamlWarning, match=r"^found duplicate mapping key 'a' \(at line 3, column 1\)$"):
            assert loads(self.TEXT) == {"a": 3, "b": 2}

    def test_none_is_silent(self) -> None:
        with warnings.catch_warnings():
            warnings.simplefilter("error")
            assert loads(self.TEXT, on_warning=None) == {"a": 3, "b": 2}

    def test_named(self) -> None:
        got: list[YamlWarning] = []

        _ = loads(self.TEXT, name="foo.yml", on_warning=got.append)

        assert str(got[0]) == "foo.yml: found duplicate mapping key 'a' (at line 3, column 1)"
        assert got[0].name == "foo.yml"

    def test_callback(self) -> None:
        got: list[YamlWarning] = []

        _ = loads(self.TEXT, on_warning=got.append)

        assert [(w.kind, w.line, w.column) for w in got] == [("duplicate_key", 3, 1)]
        assert isinstance(got[0], UserWarning)

    def test_callback_raising(self) -> None:
        def refuse(warning: YamlWarning) -> None:
            raise warning

        with pytest.raises(YamlWarning, match="duplicate mapping key"):
            _ = loads(self.TEXT, on_warning=refuse)

    def test_filter_as_error(self) -> None:
        with warnings.catch_warnings():
            warnings.simplefilter("error", YamlWarning)
            with pytest.raises(YamlWarning):
                _ = loads(self.TEXT)


class TestSelect:
    def test_only_the_keys_asked_for(self) -> None:
        text = "short_description: Hi\noptions: &o {a: {b: [1]}}\nversion_added: 2.10\nalias: *o\n"

        assert select(text, ["alias", "short_description", "version_added", "missing"]) == {
            "short_description": "Hi",
            "version_added": "2.10",
            "alias": {"a": {"b": [1]}},
        }

    def test_any_iterable(self) -> None:
        assert select("a: 1\nb: 2\n", (k for k in ("b",))) == {"b": 2}
        assert select("a: 1\nb: 2\n", {"a"}) == {"a": 1}

    def test_not_a_str(self) -> None:
        with pytest.raises(TypeError, match="keys must be an iterable of strings, not a str"):
            _ = select("a: 1\n", "a")

    def test_not_a_mapping(self) -> None:
        assert select("- a\n", ["a"]) is None
        assert select("", ["a"]) is None

    def test_still_checks_the_rest(self) -> None:
        with pytest.raises(YamlError):
            _ = select("a: 1\nb: [\n", ["a"])

    def test_warns_about_keys_asked_for(self) -> None:
        got: list[YamlWarning] = []

        assert select("a: 1\na: 2\nb: 1\nb: 2\n", ["a"], on_warning=got.append) == {"a": 2}
        assert [w.line for w in got] == [2]
