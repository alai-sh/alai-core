# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""Collection versions and version requirements, compared as ansible-galaxy compares them."""

from ._native import SemverVersion as Version
from ._native import SemverVersionReq as VersionReq

__all__ = ["Version", "VersionReq"]
