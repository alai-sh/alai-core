# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Collection artifacts, extracted safely as ``ansible-galaxy collection install`` extracts them, and the parts of a
Python sdist, such as ansible-core's, that extraction reads."""

from ._native import ExtractError
from ._native import artifact_extract as extract
from ._native import artifact_extract_sdist as extract_sdist

__all__ = ["ExtractError", "extract", "extract_sdist"]
