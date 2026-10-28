# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""TOML 1.1, read as ``tomllib`` reads it, written with a fixed layout, and edited keeping a document's layout."""

from ._native import TomlDocument as Document
from ._native import TomlEditError, TomlError
from ._native import toml_dumps as dumps
from ._native import toml_loads as loads
from ._native import toml_read as read

__all__ = ["Document", "TomlEditError", "TomlError", "dumps", "loads", "read"]
