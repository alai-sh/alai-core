# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Collection artifacts for tests: tarballs laid out as ``ansible-galaxy collection build`` writes them."""

from __future__ import annotations

import collections.abc as _c
import contextlib
import gzip
import hashlib
import io
import json
import tarfile
from pathlib import Path


@contextlib.contextmanager
def _gzipped_tar(buffer: io.BytesIO) -> _c.Generator[tarfile.TarFile]:
    """A tarball written gzipped to ``buffer``, the same bytes every time: ``w:gz`` stamps the time into the gzip
    header, so a tarball built again a second later would have another checksum than the one a lock recorded."""
    with gzip.GzipFile(fileobj=buffer, mode="wb", mtime=0) as gz, tarfile.open(fileobj=gz, mode="w") as tar:
        yield tar


Member = tuple[tarfile.TarInfo, "bytes | None"]
"""One member of a tarball, in the order it's stored: its header and, for a regular file, its content."""


def tar_file(name: str, data: bytes, mode: int = 0o644) -> Member:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    info.mode = mode
    return info, data


def tar_dir(name: str, mode: int = 0o755) -> Member:
    info = tarfile.TarInfo(name)
    info.type = tarfile.DIRTYPE
    info.mode = mode
    return info, None


def tar_symlink(name: str, target: str) -> Member:
    info = tarfile.TarInfo(name)
    info.type = tarfile.SYMTYPE
    info.linkname = target
    return info, None


def tar_hardlink(name: str, target: str) -> Member:
    info = tarfile.TarInfo(name)
    info.type = tarfile.LNKTYPE
    info.linkname = target
    return info, None


def tar_special(name: str, kind: bytes) -> Member:
    """A device or FIFO: ``tarfile.CHRTYPE``, ``BLKTYPE`` or ``FIFOTYPE``."""
    info = tarfile.TarInfo(name)
    info.type = kind
    return info, None


def build_tarball(path: Path, members: list[Member]) -> tuple[str, int]:
    """Write exactly ``members``, in order, as a gzipped tarball to ``path`` and return its SHA-256 and size; a name
    given twice is stored twice, as ``tar -r`` appends one."""
    buffer = io.BytesIO()
    with _gzipped_tar(buffer) as tar:
        for info, data in members:
            tar.addfile(info, io.BytesIO(data) if data is not None else None)
    data = buffer.getvalue()
    _ = path.write_bytes(data)
    return _sha256(data), len(data)


def manifest_json(name: str = "ns.coll", version: str = "1.0.0") -> bytes:
    namespace, short = name.split(".")
    return json.dumps({"collection_info": {"namespace": namespace, "name": short, "version": version}}).encode()


def files_json(*entries: dict[str, object]) -> bytes:
    return json.dumps({"files": [{"name": ".", "ftype": "dir"}, *entries], "format": 1}).encode()


def listed(name: str, data: bytes | None = None, *, sha256: str | None = None) -> dict[str, object]:
    """A FILES.json file entry, its checksum that of ``data``, or ``sha256``."""
    return {"name": name, "ftype": "file", "chksum_type": "sha256", "chksum_sha256": sha256 or _sha256(data or b"")}


def listed_dir(name: str) -> dict[str, object]:
    return {"name": name, "ftype": "dir", "chksum_type": None, "chksum_sha256": None}


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()
