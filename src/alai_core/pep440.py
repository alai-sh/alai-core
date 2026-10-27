# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""ansible-core versions and ``requires_ansible`` specifiers, which follow PEP 440 rather than SemVer."""

from ._native import Pep440SpecifierSet as SpecifierSet
from ._native import Pep440Version as Version

__all__ = ["SpecifierSet", "Version"]
