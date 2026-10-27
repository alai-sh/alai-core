# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

import collections.abc as _c

class SemverVersion:
    """A collection version, SemVer 2.0.0 kept as written. Versions are equal by text. They're ordered by precedence
    and then by text, so two that differ only in build metadata still have an order."""

    def __init__(self, text: str, /) -> None: ...
    @property
    def text(self) -> str:
        """As written, build metadata included."""
    @property
    def is_prerelease(self) -> bool:
        """Whether it has a pre-release part, such as ``-rc.1``."""
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __lt__(self, other: SemverVersion, /) -> bool: ...
    def __le__(self, other: SemverVersion, /) -> bool: ...
    def __gt__(self, other: SemverVersion, /) -> bool: ...
    def __ge__(self, other: SemverVersion, /) -> bool: ...

class SemverVersionReq:
    """The versions a requirement accepts. Every clause must hold, and no clauses accepts any version. Requirements are
    equal by clauses, so ``>= 1`` and ``>=1.0.0`` are the same requirement.

    ``SemverVersionReq(text)`` reads ``alai.toml``'s strict form. That's ``*``, or comma-separated clauses of an
    operator (``==``, ``!=``, ``>=``, ``<=``, ``>``, ``<``) and a version that may leave out its minor and patch
    parts."""

    def __init__(self, text: str, /) -> None: ...
    @classmethod
    def from_galaxy(cls, text: str, /) -> SemverVersionReq:
        """A requirement as ansible-galaxy reads one, as an index's ``deps[].req`` and ``MANIFEST.json`` hold them.
        There's no whitespace, a bare version means ``==``, and ``*`` as any clause matches anything. Versions are
        read loosely and then cut to three parts, so ``1.2.3.4`` means ``1.2.3``."""
    @property
    def text(self) -> str:
        """As written, for messages."""
    @property
    def clauses(self) -> tuple[tuple[str, SemverVersion], ...]:
        """``(operator, version)`` pairs, with ansible-galaxy's ``=`` and bare versions as ``==``."""
    def contains(self, version: SemverVersion, /) -> bool: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

class Pep440Version:
    """A PEP 440 version, such as ``2.19.0`` or ``2.19.0rc1``. Equal when they compare equal, so ``1.0`` is
    ``1.0.0``."""

    def __init__(self, text: str, /) -> None: ...
    @property
    def text(self) -> str:
        """As written, without surrounding whitespace."""
    @property
    def is_prerelease(self) -> bool:
        """Whether it has a pre-release or dev part, such as ``rc1`` or ``.dev0``."""
    @property
    def is_postrelease(self) -> bool:
        """Whether it has a post-release part, such as ``.post1``."""
    @property
    def base(self) -> Pep440Version:
        """The epoch and release alone, so ``2.19.0`` for ``2.19.0rc1``."""
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __lt__(self, other: Pep440Version, /) -> bool: ...
    def __le__(self, other: Pep440Version, /) -> bool: ...
    def __gt__(self, other: Pep440Version, /) -> bool: ...
    def __ge__(self, other: Pep440Version, /) -> bool: ...

class Pep440SpecifierSet:
    """Comma-separated specifiers, all of which must hold, such as ``>=2.17,<2.20``."""

    def __init__(self, text: str, /) -> None: ...
    @property
    def text(self) -> str:
        """As written, for messages."""
    def contains(self, version: Pep440Version, /) -> bool:
        """Whether ansible-core ``version`` meets every specifier. It's compared as ansible-core does, by its base
        version, so ``2.19.0rc1`` meets ``>=2.19``."""
    def admits_lower_bound(self, other: Pep440SpecifierSet, /) -> bool:
        """Whether this set starts no later than ``other``, by lower bounds alone. A collection's ``requires_ansible``
        admits the project's when it does. Either set without a lower bound admits the other."""
    def narrower_than(self, other: Pep440SpecifierSet, /) -> bool:
        """Whether this set's upper bound ends inside ``other``'s range, so ``other`` reaches versions this one
        doesn't."""
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

class NewerFormatError(ValueError):
    """A file needs a newer reader. ``found`` lists the formats or versions it offers, and ``supported`` is the newest
    read."""

    found: tuple[int, ...]
    supported: int

class IndexConfig:
    """An index's ``config.json``, with its paths as given, relative to the index root."""

    def __init__(
        self,
        *,
        index: str,
        dl: str,
        collections: str | None = None,
        plugins: str | None = None,
        moved_to: str | None = None,
    ) -> None: ...
    @classmethod
    def parse(cls, data: bytes, /) -> IndexConfig:
        """The highest format offered that's supported. Raises ``NewerFormatError`` if every one is newer."""
    @property
    def format(self) -> int:
        """The highest format number offered that's supported."""
    @property
    def index(self) -> str:
        """The template for each collection's index file."""
    @property
    def collections(self) -> str | None:
        """Where ``collections.json`` is, or None if the index has no listing."""
    @property
    def plugins(self) -> str | None:
        """Where the plugin index is, or None if the index has none."""
    @property
    def dl(self) -> str:
        """The template for artifact URLs."""
    @property
    def moved_to(self) -> str | None:
        """The index root the index has moved to, or None if it hasn't moved."""
    @property
    def hashed(self) -> bool:
        """Whether index files are found by their SHA-256, which only ``collections.json`` gives."""
    def index_path(self, *, name: str, sha256: str | None = None) -> str:
        """Where collection ``name``'s index file is, relative to the index root.

        ``sha256`` is the SHA-256 of the index file itself, from its entry in ``collections.json``. When ``hashed`` is
        True it's required, and leaving it out raises ValueError. Otherwise the template doesn't use it, and it's
        ignored."""
    def artifact_path(self, *, name: str, version: SemverVersion, sha256: str) -> str:
        """Where an artifact is, from the ``dl`` template. ``sha256`` is the artifact's, as an index line gives it."""
    def to_json(self) -> str: ...
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionEntry:
    """A collection's entry in ``collections.json``."""

    def __init__(self, updated: str, sha256: str | None = None) -> None: ...
    @property
    def updated(self) -> str:
        """When its index file last changed, or empty if the index didn't say."""
    @property
    def sha256(self) -> str | None:
        """The SHA-256 of its index file, or None if the index doesn't publish one."""
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollections:
    """An index's ``collections.json``, each collection's entry by ``namespace.name``."""

    def __init__(self, collections: dict[str, IndexCollectionEntry], /) -> None: ...
    @classmethod
    def parse(cls, data: bytes, /) -> IndexCollections:
        """A missing or malformed ``updated`` or ``sha256`` is left out rather than an error. Raises
        ``NewerFormatError`` for a newer ``v``."""
    def get(self, name: str, /) -> IndexCollectionEntry | None: ...
    def __contains__(self, name: str, /) -> bool: ...
    def __len__(self) -> int: ...
    def __iter__(self) -> _c.Iterator[str]:
        """Every collection's name, in sorted order."""
    def to_json(self) -> str: ...
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionVersion:
    """One line of an index file, one version of a collection.

    A dependency or ``requires_ansible`` that can't be read leaves the version unusable rather than unreadable.
    ``problem`` says why, and ``deps`` and ``requires_ansible`` raise ValueError rather than give part of it."""

    def __init__(
        self,
        *,
        name: str,
        version: SemverVersion,
        sha256: str,
        size: int,
        published: str,
        deps: _c.Sequence[tuple[str, str]] = (),
        requires_ansible: str | None = None,
        yanked: bool = False,
        deprecated: bool = False,
    ) -> None:
        """``deps`` holds each dependency's name and requirement as ``MANIFEST.json`` gave them."""
    @property
    def name(self) -> str:
        """The collection's full name, such as ``community.general``."""
    @property
    def version(self) -> SemverVersion:
        """This version of the collection."""
    @property
    def deps(self) -> tuple[tuple[str, SemverVersionReq], ...]:
        """Each dependency's name and requirement, in the order listed. Raises ValueError if any can't be read."""
    @property
    def sha256(self) -> str:
        """The artifact's SHA-256, in lowercase hex."""
    @property
    def size(self) -> int:
        """The artifact's size in bytes."""
    @property
    def requires_ansible(self) -> Pep440SpecifierSet | None:
        """The ansible-core versions it supports, or None for any. Raises ValueError if it can't be read."""
    @property
    def yanked(self) -> bool:
        """Whether the version is yanked, so it's only used when locked."""
    @property
    def published(self) -> str:
        """When it was first published at its origin, an RFC 3339 time in UTC."""
    @property
    def deprecated(self) -> bool:
        """Whether the version is deprecated."""
    @property
    def problem(self) -> str | None:
        """Why this version can't be used, such as a dependency requirement ansible-galaxy can't read, or None."""
    def to_json(self, *, allow_unreadable: bool = False) -> str:
        """The line as the index stores it. A line with a ``problem`` raises ValueError.

        ``allow_unreadable`` writes it anyway. Only a mirror that must publish exactly what authors published, such as
        galaxy-sync, should set it. Any other index should have the input fixed instead."""
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionIndex:
    """One collection's index file, its lines in the file's order, which is publication order rather than version
    order."""

    def __init__(self, name: str, lines: _c.Sequence[IndexCollectionVersion], /) -> None: ...
    @classmethod
    def parse(cls, data: bytes, name: str, /) -> IndexCollectionIndex:
        """Read collection ``name``'s index file. A line that can't be read is skipped, with its number and why in
        ``invalid``, and one whose ``v`` is newer than this reads is counted in ``newer``."""
    @property
    def name(self) -> str:
        """The collection the file is for."""
    @property
    def lines(self) -> tuple[IndexCollectionVersion, ...]:
        """Each line that could be read, one per version, in the file's order."""
    @property
    def newer(self) -> int:
        """How many lines were skipped because their ``v`` is newer than this reads."""
    @property
    def invalid(self) -> tuple[tuple[int, str], ...]:
        """Each line that couldn't be read, by number from 1, and why."""
    def to_jsonl(self, *, allow_unreadable: bool = False) -> str:
        """The file as the index stores it, a line each, as ``CollectionVersion.to_json`` writes them."""
