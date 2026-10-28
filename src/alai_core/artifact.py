# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Collection artifacts, extracted safely as ``ansible-galaxy collection install`` extracts them."""

from ._native import ExtractError
from ._native import artifact_extract as extract

__all__ = ["ExtractError", "extract"]
