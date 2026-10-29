# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""YAML read as Ansible reads it through PyYAML, into plain values."""

from ._native import YamlError, YamlWarning
from ._native import yaml_loads as loads
from ._native import yaml_select as select

__all__ = ["YamlError", "YamlWarning", "loads", "select"]
