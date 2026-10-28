# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import ast
import doctest
from pathlib import Path

from alai_core import _native

STUB = Path(_native.__file__).parent / "_native.pyi"


def test_examples_run() -> None:
    parser, runner = doctest.DocTestParser(), doctest.DocTestRunner()
    for node in ast.walk(ast.parse(STUB.read_text())):
        if isinstance(node, ast.FunctionDef | ast.ClassDef) and (doc := ast.get_docstring(node)) and ">>>" in doc:
            _ = runner.run(parser.get_doctest(doc, dict(vars(_native)), node.name, str(STUB), node.lineno))

    assert runner.failures == 0
    assert runner.tries > 0
