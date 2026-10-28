# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

import collections.abc as _c
import os

class SemverVersion:
    """A collection version, SemVer 2.0.0 kept as written.

    Versions are equal by text. They're ordered by precedence and then by text, so two that differ only in build
    metadata still have an order.

    Example:
        >>> sorted([SemverVersion("1.0.0"), SemverVersion("1.0.0-rc.1"), SemverVersion("0.9.0")])
        [Version('0.9.0'), Version('1.0.0-rc.1'), Version('1.0.0')]
    """

    def __init__(self, text: str, /) -> None:
        """Read a version.

        Args:
            text: The version, such as ``1.2.3`` or ``1.2.3-rc.1+build``.

        Raises:
            ValueError: It isn't a semantic version.
        """
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
    """The versions a requirement accepts.

    Every clause must hold, and no clauses accepts any version. Requirements are equal by clauses, so ``>= 1`` and
    ``>=1.0.0`` are the same requirement.

    Example:
        >>> SemverVersionReq(">=1.0, <2").contains(SemverVersion("1.5.0"))
        True
    """

    def __init__(self, text: str, /) -> None:
        """Read a requirement in ``alai.toml``'s strict form.

        That's ``*``, or comma-separated clauses of an operator (``==``, ``!=``, ``>=``, ``<=``, ``>``, ``<``) and a
        version that may leave out its minor and patch parts.

        Args:
            text: The requirement, such as ``>=9.1,<10``.

        Raises:
            ValueError: It isn't a requirement in this form. The message says what to write instead.
        """
    @classmethod
    def from_galaxy(cls, text: str, /) -> SemverVersionReq:
        """Read a requirement as ansible-galaxy reads one, as an index's ``deps[].req`` and ``MANIFEST.json`` hold them.

        There's no whitespace, a bare version means ``==``, and ``*`` as any clause matches anything. Versions are read
        loosely and then cut to three parts.

        Args:
            text: The requirement.

        Returns:
            The requirement.

        Raises:
            ValueError: ansible-galaxy couldn't read it either, such as ``^3.0.0``.

        Example:
            >>> SemverVersionReq.from_galaxy("1.2.3.4").clauses
            (('==', Version('1.2.3')),)
        """
    @property
    def text(self) -> str:
        """As written, for messages."""
    @property
    def clauses(self) -> tuple[tuple[str, SemverVersion], ...]:
        """``(operator, version)`` pairs, with ansible-galaxy's ``=`` and bare versions as ``==``."""
    def contains(self, version: SemverVersion, /) -> bool:
        """Whether a version meets every clause.

        Args:
            version: The version.

        Returns:
            True if it does. Precedence is compared, so build metadata is ignored.
        """
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

class Pep440Version:
    """A PEP 440 version, such as ``2.19.0`` or ``2.19.0rc1``.

    Versions are equal when they compare equal, so ``1.0`` is ``1.0.0``.

    Example:
        >>> Pep440Version("2.19.0rc1").base
        Version('2.19.0')
    """

    def __init__(self, text: str, /) -> None:
        """Read a version.

        Args:
            text: The version.

        Raises:
            ValueError: It isn't a PEP 440 version, or has a local part such as ``+local``.
        """
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
    """Comma-separated specifiers, all of which must hold, such as ``>=2.17,<2.20``.

    Example:
        >>> Pep440SpecifierSet(">=2.19").contains(Pep440Version("2.19.0rc1"))
        True
    """

    def __init__(self, text: str, /) -> None:
        """Read a specifier set. Empty items are skipped, so ``>=2.17,`` is ``>=2.17``.

        Args:
            text: The specifiers, such as a collection's ``requires_ansible``.

        Raises:
            ValueError: A specifier isn't valid.
        """
    @property
    def text(self) -> str:
        """As written, for messages."""
    def contains(self, version: Pep440Version, /) -> bool:
        """Whether an ansible-core version meets every specifier, compared as ansible-core does, by its base version.

        Args:
            version: The ansible-core version.

        Returns:
            True if its base version does, so ``2.19.0rc1`` meets ``>=2.19``.
        """
    def admits_lower_bound(self, other: Pep440SpecifierSet, /) -> bool:
        """Whether this set starts no later than another, by lower bounds alone.

        A collection's ``requires_ansible`` admits the project's when it does.

        Args:
            other: The set to compare with.

        Returns:
            True if it does, or if either set has no lower bound.
        """
    def narrower_than(self, other: Pep440SpecifierSet, /) -> bool:
        """Whether this set's upper bound ends inside another's range.

        Args:
            other: The set to compare with.

        Returns:
            True if ``other`` reaches versions this one doesn't.
        """
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

class NewerFormatError(ValueError):
    """A file needs a newer reader.

    Attributes:
        found: The formats or versions the file offers.
        supported: The newest this reads.
    """

    found: tuple[int, ...]
    supported: int

class IndexConfig:
    """An index's ``config.json``, with its paths as given, relative to the index root.

    Example:
        >>> config = IndexConfig(index="index/{namespace}/{name}.jsonl", dl="https://dl.example/{sha256-checksum}")
        >>> config.index_path(name="community.general")
        'index/community/general.jsonl'
    """

    def __init__(
        self,
        *,
        index: str,
        dl: str,
        collections: str | None = None,
        plugins: str | None = None,
        moved_to: str | None = None,
    ) -> None:
        """Make a config to write.

        Args:
            index: The template for each collection's index file.
            dl: The template for artifact URLs.
            collections: Where ``collections.json`` is, or None for an index with no listing.
            plugins: Where the plugin index is, or None for an index with none.
            moved_to: The index root the index has moved to, or None.

        Raises:
            ValueError: A template is invalid, or the index template finds files by SHA-256 with no listing.
        """
    @classmethod
    def parse(cls, data: bytes, /) -> IndexConfig:
        """Read ``config.json``, choosing the highest format offered that's supported.

        Args:
            data: The file.

        Returns:
            The config.

        Raises:
            NewerFormatError: Every format offered is newer than this reads.
            ValueError: The file isn't valid.
        """
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
        """Where a collection's index file is, relative to the index root.

        Args:
            name: The collection's full name, such as ``community.general``.
            sha256: The SHA-256 of the index file itself, from its entry in ``collections.json``. Required when
                ``hashed`` is True. Otherwise the template doesn't use it, and it's ignored.

        Returns:
            The path, with the template filled in.

        Raises:
            ValueError: The name isn't a valid collection name, or ``hashed`` is True and ``sha256`` is None.
        """
    def artifact_path(self, *, name: str, version: SemverVersion, sha256: str) -> str:
        """Where an artifact is, from the ``dl`` template.

        Args:
            name: The collection's full name.
            version: The version, percent-encoded into the URL.
            sha256: The artifact's SHA-256, as an index line gives it.

        Returns:
            The URL, with the template filled in.

        Raises:
            ValueError: The name isn't a valid collection name.

        Example:
            >>> IndexConfig(index="i/{namespace}/{name}", dl="{namespace}/{name}/{version}.tar.gz").artifact_path(
            ...     name="ns.coll", version=SemverVersion("1.0.0+b"), sha256="a" * 64
            ... )
            'ns/coll/1.0.0%2Bb.tar.gz'
        """
    def to_json(self) -> str:
        """The file as galaxy-sync writes it, indented by two spaces."""
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionEntry:
    """A collection's entry in ``collections.json``."""

    def __init__(self, updated: str, sha256: str | None = None) -> None:
        """Make an entry to write.

        Args:
            updated: When its index file last changed, an RFC 3339 time in UTC such as ``2026-10-05T17:30:02Z``.
            sha256: The SHA-256 of its index file, or None.

        Raises:
            ValueError: ``updated`` isn't such a time, or ``sha256`` isn't lowercase hex.
        """
    @property
    def updated(self) -> str:
        """When its index file last changed, or empty if the index didn't say."""
    @property
    def sha256(self) -> str | None:
        """The SHA-256 of its index file, or None if the index doesn't publish one."""
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollections:
    """An index's ``collections.json``, each collection's entry by ``namespace.name``."""

    def __init__(self, collections: dict[str, IndexCollectionEntry], /) -> None:
        """Make a listing to write.

        Args:
            collections: Each collection's entry, by full name.

        Raises:
            ValueError: A name isn't a valid collection name.
        """
    @classmethod
    def parse(cls, data: bytes, /) -> IndexCollections:
        """Read ``collections.json``. A missing or malformed ``updated`` or ``sha256`` is left out rather than an error.

        Args:
            data: The file.

        Returns:
            The listing.

        Raises:
            NewerFormatError: Its ``v`` is newer than this reads.
            ValueError: The file isn't valid.
        """
    def get(self, name: str, /) -> IndexCollectionEntry | None:
        """A collection's entry, or None if it isn't listed."""
    def __contains__(self, name: str, /) -> bool: ...
    def __len__(self) -> int: ...
    def __iter__(self) -> _c.Iterator[str]:
        """Every collection's name, in sorted order."""
    def to_json(self) -> str:
        """The file as galaxy-sync writes it, indented by one space, its collections sorted.

        Raises:
            ValueError: A name or ``updated`` read leniently isn't valid to write.
        """
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionVersion:
    """One line of an index file, one version of a collection.

    A dependency or ``requires_ansible`` that can't be read leaves the version unusable rather than unreadable.
    ``problem`` says why, and ``deps`` and ``requires_ansible`` raise ValueError rather than give part of it.
    """

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
        """Make a line to write.

        Args:
            name: The collection's full name.
            version: The version.
            sha256: The artifact's SHA-256, in lowercase hex.
            size: The artifact's size in bytes.
            published: When it was first published at its origin, an RFC 3339 time in UTC.
            deps: Each dependency's name and requirement, as ``MANIFEST.json`` gave them.
            requires_ansible: The ansible-core versions it supports, as ``meta/runtime.yml`` gave them, or None.
            yanked: Whether it's yanked.
            deprecated: Whether it's deprecated.

        Raises:
            ValueError: The name, ``sha256`` or ``published`` isn't valid. An unreadable dependency or
                ``requires_ansible`` isn't refused here, but by ``to_json``.
        """
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
        """The line as galaxy-sync writes it, compact and in the spec's field order.

        Args:
            allow_unreadable: Write it even with a ``problem``. Only a mirror that must publish exactly what authors
                published, such as galaxy-sync, should set it. Any other index should have the input fixed instead.

        Returns:
            The line, without a newline.

        Raises:
            ValueError: It has a ``problem`` and ``allow_unreadable`` isn't set, or a field read leniently isn't valid
                to write.
        """
    def __eq__(self, other: object, /) -> bool: ...

class IndexCollectionIndex:
    """One collection's index file, its lines in the file's order, which is publication order rather than version
    order."""

    def __init__(self, name: str, lines: _c.Sequence[IndexCollectionVersion], /) -> None:
        """Make a file to write.

        Args:
            name: The collection's full name.
            lines: Its lines, in the order to write them.

        Raises:
            ValueError: A line is for another collection, or a version is listed twice.
        """
    @classmethod
    def parse(cls, data: bytes, name: str, /) -> IndexCollectionIndex:
        """Read a collection's index file.

        A line that can't be read is skipped, with its number and why in ``invalid``, and one whose ``v`` is newer than
        this reads is counted in ``newer``.

        Args:
            data: The file.
            name: The collection it's for, which every line must name.

        Returns:
            The file.
        """
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
        """The file as the index stores it, a line each, as ``IndexCollectionVersion.to_json`` writes them.

        Args:
            allow_unreadable: As for ``IndexCollectionVersion.to_json``.

        Returns:
            The file, each line ending in a newline.

        Raises:
            ValueError: As for ``IndexCollectionVersion.to_json``.
        """

class PluginEntry:
    """One line of the plugin index, a plugin, role or playbook a collection has or had."""

    def __init__(
        self,
        type: str,  # noqa: A002
        name: str,
        collection: str,
        *,
        since: str | None = None,
        until: str | None = None,
        short_description: str | None = None,
        routing: _c.Mapping[str, object] | None = None,
    ) -> None:
        """Make a line to write.

        Args:
            type: The type, such as ``module`` or ``filter``.
            name: The short name, such as ``user``.
            collection: The collection that has or had it.
            since: The lowest version of the collection that has it.
            until: The version it was removed in, or None if the latest still has it.
            short_description: One line from its documentation.
            routing: Its entry in the collection's ``plugin_routing``, copied as it is.

        Raises:
            ValueError: ``type`` or ``name`` is empty, the collection name isn't valid, or ``routing`` isn't JSON.
        """
    @classmethod
    def parse(cls, line: bytes, /) -> PluginEntry | None:
        """Read one line. A plugin index is a search aid, so a bad line is skipped rather than an error.

        Args:
            line: The line.

        Returns:
            The plugin, or None if the line can't be read or needs a newer reader.

        Example:
            >>> PluginEntry.parse(b'{"v": 1, "type": "module", "name": "copy", "collection": "ansible.builtin"}').fqcn
            'ansible.builtin.copy'
        """
    @property
    def type(self) -> str:
        """The type, such as ``module`` or ``filter``, which may be one this code doesn't know."""
    @property
    def name(self) -> str:
        """The short name, such as ``user``."""
    @property
    def collection(self) -> str:
        """The collection that has or had it, ``ansible.builtin`` for ansible-core's own."""
    @property
    def fqcn(self) -> str:
        """The full name, such as ``ansible.builtin.user``."""
    @property
    def since(self) -> str | None:
        """The lowest version of the collection that has it."""
    @property
    def until(self) -> str | None:
        """The version it was removed in, or None if the latest still has it."""
    @property
    def short_description(self) -> str | None:
        """One line from its documentation."""
    @property
    def routing(self) -> dict[str, object] | None:
        """Its entry in the collection's ``plugin_routing``, as the index copied it."""
    @property
    def line(self) -> bytes | None:
        """The line it was read from, unknown fields included, or None if it wasn't read."""
    def to_json(self) -> str:
        """The line as the index stores it, compact, with the keys within ``routing`` sorted.

        Raises:
            ValueError: A collection name read leniently isn't valid to write.
        """
    def __eq__(self, other: object, /) -> bool: ...

class Plugins:
    """A plugin index's ``plugins.jsonl``, searched without parsing the lines that can't match.

    Example:
        >>> plugins = Plugins(b'{"v": 1, "type": "module", "name": "win_user", "collection": "ansible.windows"}\\n')
        >>> [p.fqcn for p in plugins.search([b"user"])]
        ['ansible.windows.win_user']
    """

    def __init__(self, data: bytes, /) -> None:
        """Hold a plugin index to search.

        Args:
            data: The file. It's searched as it is, without being copied.
        """
    def search(self, tokens: _c.Sequence[bytes] = ()) -> list[PluginEntry]:
        """Find the lines that contain every token.

        Args:
            tokens: The bytes each line must contain. The first is searched for, so it should be the rarest.

        Returns:
            Every readable line that contains them all, or every readable line if there are none.
        """
    def named(self, names: _c.Collection[str], /) -> list[PluginEntry]:
        """Find the lines for plugins with any of the given names, in one pass.

        Args:
            names: The short names.

        Returns:
            Every readable line whose ``name`` is one of them.
        """
    def names(self) -> set[str]:
        """Every plugin's short name, read without parsing the lines, for suggestions."""

class ExtractError(Exception):
    """A tarball can't be read, or its collection can't be extracted safely. ``str()`` says why.

    Attributes:
        kind: What's wrong, one of ``corrupt``, ``missing``, ``not_file``, ``not_dir``, ``path``, ``link``,
            ``escape``, ``unreadable``, ``checksum``, ``too_large``, ``json``, ``not_object``, ``manifest``,
            ``manifest_fields``, ``files_list``, ``files_entry``, ``files_checksum``, ``files_special`` or
            ``files_twice``.
        entry: The entry it's about, or empty.
        detail: More about it, or empty. For ``corrupt`` and ``json`` it's why, for ``link`` the link's target, for
            ``too_large`` the limit, for ``manifest`` the collection it names, and for ``files_entry`` the entry as
            JSON.
    """

    kind: str
    entry: str
    detail: str

def artifact_extract(
    tarball: str | os.PathLike[str],
    dest: str | os.PathLike[str],
    *,
    limit: int = 0,
    expected: tuple[str, str, str] | None = None,
) -> tuple[str, str, str]:
    """Extract a collection artifact as ``ansible-galaxy collection install`` does, with the GIL released.

    Only ``MANIFEST.json``, ``FILES.json`` and what ``FILES.json`` lists are written, the last member of each name, as
    ``tar -r`` appends a replacement. Both files and every listed entry are checked before anything is written, and
    each file is checked against its SHA-256 as it's written. Every path and symlink must stay inside ``dest``, by name
    and on disk, as one symlink can lead another out.

    Files are ``0644``, or ``0755`` if the tarball marks them executable, and directories ``0755``, masked by the
    process umask. On Windows, modes aren't set, and a name Windows can't hold safely, such as ``aux.py``, is
    refused as ``path``. A symlink is made as a link to a directory or a file as ``FILES.json`` lists it. If Windows
    won't make links, as without Developer Mode, what it points to is copied instead.

    Args:
        tarball: The artifact.
        dest: The directory to extract it into, which mustn't exist yet.
        limit: The most it may unpack to, in bytes, counting what a hard link copies and a symlink's checksum reads.
            0 is no limit.
        expected: The namespace, name and version ``MANIFEST.json`` must give, or None to take what it gives.

    Returns:
        The namespace, name and version ``MANIFEST.json`` gives.

    Raises:
        ExtractError: The tarball can't be read, or can't be extracted safely.
        OSError: The filesystem's own error. ``filename`` is where it happened.
    """

class TomlError(ValueError):
    """Text that isn't valid TOML.

    ``str()`` says why and where, such as ``... (at line 3, column 7)``, the column in characters, as ``tomllib`` says
    it.
    """

class TomlEditError(Exception):
    """An edit ``TomlDocument`` can't make.

    That's an edit to a key that isn't there or isn't the right type, or one whose text, read back, wouldn't be the
    document with only that change made. ``str()`` says which key and why.
    """

def toml_loads(text: str, /) -> dict[str, object]:
    """Read TOML 1.1 text as ``tomllib.loads`` reads it.

    Unlike ``tomllib``, it skips a byte order mark and keeps a CRLF inside a multi-line string as written.

    Args:
        text: The TOML document.

    Returns:
        The document, tables as dicts, arrays as lists, and dates and times as ``datetime`` objects.

    Raises:
        TomlError: The text isn't valid TOML.

    Example:
        >>> toml_loads('name = "demo"\\n[project]\\ncollections = ["a.a"]\\n')
        {'name': 'demo', 'project': {'collections': ['a.a']}}
    """

def toml_read(path: str | os.PathLike[str], /) -> dict[str, object]:
    """Read a TOML file as ``toml_loads`` reads text, with the GIL released while it's read.

    Args:
        path: The file to read.

    Returns:
        The document, as ``toml_loads`` gives it.

    Raises:
        TomlError: The file isn't UTF-8, such as ``not UTF-8 (at line 3, column 7)``, or isn't valid TOML.
        OSError: The file can't be read. ``filename`` is ``path``.
    """

def toml_dumps(table: _c.Mapping[str, object], /, *, width: int) -> str:
    """Write a mapping as TOML, laid out the same way every time.

    How each value is written depends on its type and where it is. Keys keep the mappings' order.

    - A mapping in ``table`` is written as a ``[key]`` header line and then its own keys.
    - A non-empty list in ``table`` whose items are all mappings gets a ``[[key]]`` header line for each item where
      each header follows a blank line.
    - Every other value in ``table`` is a ``key = value`` line, written before the first header.
    - Inside a table, a mapping of only strings, numbers and booleans is an inline table, ``{ k = "v" }``. Any other
      mapping is written as dotted keys, ``a.b.c = ...``.
    - A list is an array, on one line if that's at most ``width`` characters, else one item per line with a trailing
      comma.
    - ``str``, ``int``, ``float`` and ``bool`` are TOML's string, integer, float and boolean. Anything else, ``None``
      included, raises ``ValueError``.

    Args:
        table: The document, its keys and values as described above.
        width: The longest an array can be on one line, in characters.

    Returns:
        The TOML text.

    Raises:
        ValueError: A value has a type TOML can't write.

    Example:
        >>> print(toml_dumps(
        ...     {
        ...         "version": 1,
        ...         "tags": ["a", "b"],
        ...         "project": {
        ...             "name": "demo",
        ...             "owner": {"name": "me", "admin": True},
        ...             "source": {"git": {"url": "https://x"}},
        ...         },
        ...         "collection": [{"name": "a.a", "dependencies": ["b.b", "c.c"]}, {"name": "b.b"}],
        ...     },
        ...     width=12,
        ... ), end="")
        version = 1
        tags = ["a", "b"]
        <BLANKLINE>
        [project]
        name = "demo"
        owner = { name = "me", admin = true }
        source.git = { url = "https://x" }
        <BLANKLINE>
        [[collection]]
        name = "a.a"
        dependencies = [
            "b.b",
            "c.c",
        ]
        <BLANKLINE>
        [[collection]]
        name = "b.b"
    """

class TomlDocument:
    """A TOML document being edited.

    Only what's changed is rewritten. An array being edited is laid out one item per line with a trailing comma,
    comments are kept with the lines they were on, and a document with CRLF lines keeps them.

    Every edit is checked. Its text, read back, must be the document as it was with only that change made, or the edit
    raises ``TomlEditError`` and the document is left as the last good edit had it. A path is a key path, such as
    ``("project", "groups", "test", "collections")``, through tables, dotted keys or inline tables alike.

    Example:
        >>> doc = TomlDocument('[project]\\ncollections = [\\n    "a.a",  # pinned\\n    "c.c",\\n]\\n')
        >>> doc.insert(("project", "collections"), 1, "b.b")
        >>> print(doc, end="")
        [project]
        collections = [
            "a.a",  # pinned
            "b.b",
            "c.c",
        ]
    """

    def __init__(self, text: str, /) -> None:
        """Read a document to edit.

        Args:
            text: The TOML document.

        Raises:
            TomlError: The text isn't valid TOML.
        """
    @classmethod
    def from_path(cls, path: str | os.PathLike[str], /) -> TomlDocument:
        """Read a TOML file to edit, as ``toml_read`` reads one, its bytes as they are so its newlines are kept.

        Args:
            path: The file to read.

        Returns:
            The document.

        Raises:
            TomlError: The file isn't UTF-8 or isn't valid TOML.
            OSError: The file can't be read.
        """
    def __str__(self) -> str:
        """The text, as given until an edit is made."""
    def data(self) -> dict[str, object]:
        """The document as ``toml_loads`` gives it, with the edits made."""
    def values(self, path: _c.Sequence[str], /) -> list[str]:
        """The array of strings at a key.

        Args:
            path: The key's path.

        Returns:
            The strings, or an empty list if there's no such key.

        Raises:
            TomlEditError: The key isn't an array of strings.
        """
    def insert(self, path: _c.Sequence[str], index: int, value: str, /) -> None:
        """Insert a string into an array, creating the array if there's no such key.

        A new array is created with the tables it's in. The table holding it gets a header, such as
        ``[project.groups.test]``, after the last table before it in the document's order. The tables above it don't
        get one.

        Args:
            path: The array's key path.
            index: Where to insert it, as ``list.insert`` takes one.
            value: The string.

        Raises:
            TomlEditError: The key isn't an array, something on the path isn't a table, or the edit changes more than
                it should.
        """
    def replace(self, path: _c.Sequence[str], index: int, value: str, /) -> None:
        """Replace an item in an array, keeping its comments.

        Args:
            path: The array's key path.
            index: The item's index.
            value: The string to put in its place.

        Raises:
            TomlEditError: There's no such array, or the edit changes more than it should.
        """
    def remove(self, path: _c.Sequence[str], index: int, /) -> None:
        """Remove an item from an array.

        A comment on its line goes on a line of its own, and an emptied array is ``[]``.

        Args:
            path: The array's key path.
            index: The item's index.

        Raises:
            TomlEditError: There's no such array, or the edit changes more than it should.
        """
    def set_table(self, path: _c.Sequence[str], value: _c.Mapping[str, str | bool], /) -> None:
        """Set a key to an inline table.

        It's set in place, keeping a comment after it, or added at the end of its table, created as ``insert`` creates
        one.

        Args:
            path: The key's path.
            value: The table, in the order to write it.

        Raises:
            TomlEditError: Something on the path isn't a table, or the edit changes more than it should.
        """
    def remove_key(self, path: _c.Sequence[str], /) -> None:
        """Remove a key and the comments on its line and above it, leaving its table, even if empty.

        Args:
            path: The key's path.

        Raises:
            TomlEditError: There's no such key, or the edit changes more than it should.
        """

class YamlError(ValueError):
    """The text isn't YAML Ansible could read. ``str()`` says why and where, such as ``... (at line 2, column 1)``.

    Attributes:
        name: Where the document came from, as it was given, or None.
        line: The line it's on, from 1, or None if the parser didn't say.
        column: The column, from 1, or None.
    """

    name: str | None
    line: int | None
    column: int | None

class YamlWarning(UserWarning):
    """Something in a document worth knowing about that didn't stop it being read.

    Attributes:
        kind: What it is. ``duplicate_key`` is a mapping key given more than once, of which the last value is kept,
            where the key was first, as PyYAML keeps it.
        name: Where the document came from, as it was given, or None.
        line: The line it's on, from 1.
        column: The column, from 1.
    """

    kind: str
    name: str | None
    line: int
    column: int

def yaml_loads(text: str, /, *, name: str | None, on_warning: _c.Callable[[YamlWarning], object] | None) -> object:
    """Read a YAML document as Ansible reads it through PyYAML, with libyaml's parser, the GIL released while parsing.

    The values are plain and given as they are, with no schema. Mappings are ``dict``, sequences are ``list``, and
    scalars are ``str``. This doesn't follow YAML 1.1's or PyYAML's rules for other types, to keep it simple, except
    for a plain, untagged scalar that is one of these.

    - A boolean, as YAML 1.1 and PyYAML spell one, ``yes``, ``no``, ``on``, ``off``, ``true`` or ``false`` in any of
      PyYAML's cases. ``y`` and ``n`` stay strings, as PyYAML leaves them.
    - None, as ``~``, ``null`` or an empty value.
    - An ``int`` or ``float`` whose ``str()`` is the original text, such as ``42`` or ``2.9``.

    Booleans are kept because Ansible's docs rely on them everywhere, as in ``required: no``. None and numbers are kept
    where converting them loses nothing. Anything else stays the text its author wrote, which is what a reader of the
    source sees and almost always what was meant. So ``2.10``, ``0644`` and ``2021-01-01`` stay strings, where PyYAML
    gives ``2.1``, ``420`` and a ``date``. Keys are any of these values but ``dict`` and ``list``.

    Quoted and block scalars are strings, and so is any scalar with a tag, such as ``!!int 1`` or Ansible's
    ``!unsafe``, as it was written. A tag with nothing after it, such as ``!abc:server.tld``, was meant as text, so it
    gives the tag's text. ``<<`` merge keys are merged as PyYAML merges them, and an alias is the same object as its
    anchor. An empty document is None.

    Args:
        text: The document. A second document in it is an error, as PyYAML's ``load`` gives.
        name: Where the document came from, such as its file, or None. Each error and warning message starts with it,
            as ``name: message``.
        on_warning: Called with each ``YamlWarning`` once the document is read, or None to ignore them. If it raises,
            so does this.

    Returns:
        The document's value.

    Raises:
        YamlError: The text isn't YAML Ansible could read, nests deeper than 256, merges keys into far more than its
            text holds, or isn't valid Unicode, such as a string decoded with ``surrogateescape``.

    Example:
        >>> yaml_loads("required: no\\nversion_added: 2.10\\nmode: 0644\\nsince: 2.9\\n", name=None, on_warning=None)
        {'required': False, 'version_added': '2.10', 'mode': '0644', 'since': 2.9}
    """

def yaml_select(
    text: str,
    keys: _c.Sequence[str],
    /,
    *,
    name: str | None,
    on_warning: _c.Callable[[YamlWarning], object] | None,
) -> dict[str, object] | None:
    """Read only some top-level keys of a YAML document, skipping the work of building large values that aren't wanted.

    The values are as ``yaml_loads`` gives them, and the text is checked as fully as it is there.

    Args:
        text: The document.
        keys: The top-level keys wanted. One the document doesn't have is left out.
        name: As for ``yaml_loads``.
        on_warning: As for ``yaml_loads``, for warnings about the keys wanted.

    Returns:
        The keys found, in the document's order, or None if its top level isn't a mapping.

    Raises:
        YamlError: As for ``yaml_loads``.

    Example:
        >>> text = "short_description: Copy files\\noptions: {src: {type: path}}\\n"
        >>> yaml_select(text, ["short_description"], name=None, on_warning=None)
        {'short_description': 'Copy files'}
    """
