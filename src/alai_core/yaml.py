# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""YAML read as Ansible reads it through PyYAML, into plain values."""

import collections.abc as _c
import warnings

from ._native import YamlError, YamlWarning, yaml_loads, yaml_select

__all__ = ["YamlError", "YamlWarning", "loads", "select"]


def loads(
    text: str,
    *,
    name: str | None = None,
    on_warning: _c.Callable[[YamlWarning], object] | None = warnings.warn,
) -> object:
    """Read a YAML document as Ansible reads it. See ``alai_core._native.yaml_loads`` for the values it gives.

    Args:
        text: The document.
        name: Where the document came from, such as its file, put before each error and warning message.
        on_warning: Called with each ``YamlWarning``. By default each goes to ``warnings.warn``, so the warning filters
            apply. None ignores them.

    Returns:
        The document's value.

    Raises:
        YamlError: The text isn't YAML Ansible could read.
    """
    return yaml_loads(text, name=name, on_warning=on_warning)


def select(
    text: str,
    keys: _c.Iterable[str],
    *,
    name: str | None = None,
    on_warning: _c.Callable[[YamlWarning], object] | None = warnings.warn,
) -> dict[str, object] | None:
    """Read only the top-level ``keys`` of a YAML document, building nothing else.

    See ``alai_core._native.yaml_select`` for what it gives.

    Args:
        text: The document.
        keys: The top-level keys wanted.
        name: As for ``loads``.
        on_warning: As for ``loads``.

    Returns:
        The keys found, or None if the document's top level isn't a mapping.

    Raises:
        YamlError: The text isn't YAML Ansible could read.
    """
    return yaml_select(text, list(keys), name=name, on_warning=on_warning)
