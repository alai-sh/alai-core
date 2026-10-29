# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Ansible's documentation markup."""

from ._native import MarkupPart as Part
from ._native import markup_parse as parse

__all__ = ["Part", "parse"]
