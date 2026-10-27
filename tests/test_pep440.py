# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import pytest

from alai_core.pep440 import SpecifierSet, Version


class TestVersion:
    def test_order(self) -> None:
        ordered = [
            "1.0.dev0",
            "1.0a1.dev1",
            "1.0a1",
            "1.0b2",
            "1.0rc1",
            "1.0",
            "1.0.post1.dev0",
            "1.0.post1",
            "1.0.1",
            "2.0",
            "1!0.5",
        ]

        assert sorted(Version(v) for v in reversed(ordered)) == [Version(v) for v in ordered]

    @pytest.mark.parametrize(
        ("a", "b"),
        [("1.0", "1.0.0"), ("1.0rc1", "1.0c1"), ("1.0alpha2", "1.0a2"), ("1.0-1", "1.0.post1"), ("v1.0", "1.0")],
    )
    def test_equivalent(self, a: str, b: str) -> None:
        assert Version(a) == Version(b)
        assert hash(Version(a)) == hash(Version(b))

    @pytest.mark.parametrize("text", ["", "1.0+local", "x", "1.0.x", "2.17.*"])
    def test_invalid(self, text: str) -> None:
        with pytest.raises(ValueError, match="invalid version"):
            Version(text)

    def test_comparisons(self) -> None:
        low, high = Version("2.17"), Version("2.18rc1")

        assert low <= low < high <= high
        assert high >= high > low >= low
        assert str(Version(" 2.17 ")) == "2.17"
        assert repr(low) == "Version('2.17')"

    def test_pre_and_post_releases(self) -> None:
        assert Version("2.19.0rc1").is_prerelease
        assert Version("2.19.0.dev1").is_prerelease
        assert Version("2.19.0.post1").is_postrelease
        assert not Version("2.19.0").is_prerelease

    def test_base(self) -> None:
        assert Version("2.19.0rc1").base == Version("2.19.0")
        assert Version("1!2.0.post1").base.text == "1!2.0"


class TestSpecifierSet:
    @pytest.mark.parametrize(
        ("text", "match"),
        [
            ("2.17", "invalid specifier '2.17'"),
            (">=2.17.*", "invalid specifier"),
            ("==2.17+local", "invalid specifier"),
            ("~=2", "needs at least two release parts"),
            ("==2.17rc1.*", "can only follow a release"),
            (">=2.17,<x", "invalid specifier '<x'"),
        ],
    )
    def test_invalid(self, text: str, match: str) -> None:
        with pytest.raises(ValueError, match=match):
            SpecifierSet(text)

    @pytest.mark.parametrize(
        ("spec", "version", "expected"),
        [
            (">=2.17", "2.17.0", True),
            (">=2.17", "2.16.9", False),
            (" >= 2.15 , < 2.19 ", "2.18.3", True),
            (">=2.15,<2.19", "2.19.0", False),
            ("~=2.17", "2.20.0", True),
            ("~=2.17", "3.0.0", False),
            ("~=2.17.1", "2.17.5", True),
            ("~=2.17.1", "2.18.0", False),
            ("==2.18.*", "2.18.4", True),
            ("==2.18.*", "2.19.0", False),
            ("!=2.18.*", "2.18.0", False),
            ("==2.18", "2.18.0", True),
            ("==1!2.18.*", "2.18.0", False),
            (">2.18", "2.18.0", False),
            ("===2.18.0", "2.18.0", True),
            ("===2.18", "2.18.0", False),
            (">=2.17,", "2.17.0", True),
            ("<=2.18", "2.18.0", True),
            ("<=2.18", "2.18.1", False),
            # ansible-core compares its base version, so its own pre-releases count as the release.
            (">=2.19.0", "2.19.0rc1", True),
            ("<2.19", "2.19.0b2", False),
            (">=2.19.0b1", "2.19.0", True),
        ],
    )
    def test_contains(self, spec: str, version: str, expected: bool) -> None:
        assert SpecifierSet(spec).contains(Version(version)) is expected

    def test_empty_contains_everything(self) -> None:
        assert SpecifierSet("").contains(Version("2.9.0"))

    def test_equality(self) -> None:
        assert SpecifierSet(">=2.17, <2.20") == SpecifierSet(">=2.17.0,<2.20")
        assert SpecifierSet(">=2.17") != SpecifierSet(">2.17")
        assert SpecifierSet("==2.17.*") != SpecifierSet("==2.17")
        assert SpecifierSet("===2.17.0") == SpecifierSet("=== 2.17.0")
        assert len({SpecifierSet(">=2.17"), SpecifierSet(">= 2.17.0")}) == 1
        assert SpecifierSet(">=2.17") != ">=2.17"

    def test_text(self) -> None:
        specifiers = SpecifierSet(">=2.17, <2.20")

        assert specifiers.text == str(specifiers) == ">=2.17, <2.20"
        assert repr(specifiers) == "SpecifierSet('>=2.17, <2.20')"


class TestBounds:
    @pytest.mark.parametrize(
        ("collection", "project", "expected"),
        [
            (">=2.15", ">=2.17", True),
            (">=2.17", ">=2.17", True),
            (">=2.18", ">=2.17", False),
            (">2.17", ">=2.17", False),
            (">2.17", ">2.17", True),
            (">=2.17", ">2.17", True),
            (">=2.15,<2.16", ">=2.17", True),  # upper bounds are ignored
            ("<2.20", ">=2.17", True),
            (">=2.99", "<2.20", True),  # a project with no lower bound filters nothing
            ("~=2.16", ">=2.17", True),
            ("==2.18.*", ">=2.17", False),
        ],
    )
    def test_admits_lower_bound(self, collection: str, project: str, expected: bool) -> None:
        assert SpecifierSet(collection).admits_lower_bound(SpecifierSet(project)) is expected

    @pytest.mark.parametrize(
        ("collection", "project", "expected"),
        [
            (">=2.15", ">=2.17", False),
            ("<2.20", ">=2.17", True),
            ("<2.20", ">=2.17,<2.20", False),
            ("<2.20", ">=2.17,<=2.20", True),
            ("<=2.20", ">=2.17,<=2.20", False),
            ("<2.19", ">=2.17,<2.20", True),
            ("~=2.15", ">=2.17", True),
        ],
    )
    def test_narrower_than(self, collection: str, project: str, expected: bool) -> None:
        assert SpecifierSet(collection).narrower_than(SpecifierSet(project)) is expected
