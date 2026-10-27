# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

from __future__ import annotations

import os
import subprocess
import sys
import sysconfig
from pathlib import Path

import pytest

from alai_core import _native


def test_one_build_beside_it() -> None:
    # uv sync builds into src/alai_core/ with an ABI-specific name, so switching Pythons leaves the old build behind,
    # and 3.15 loads a stale _native.abi3.so before a new _native.abi3t.so.
    builds = sorted(p.name for p in Path(_native.__file__).parent.glob("_native*") if p.suffix in (".so", ".pyd"))

    assert builds == [Path(_native.__file__).name], (
        f"delete the stale builds of alai_core._native in src/alai_core/: {builds}"
    )


@pytest.mark.skipif(not sysconfig.get_config_var("Py_GIL_DISABLED"), reason="a free-threaded build of Python only")
def test_leaves_the_gil_off() -> None:
    # A fresh interpreter, so nothing imported earlier hides the warning.
    env = {k: v for k, v in os.environ.items() if k != "PYTHON_GIL"}
    code = "import sys, alai_core._native; print(sys._is_gil_enabled())"

    done = subprocess.run(  # noqa: S603
        [sys.executable, "-W", "error::RuntimeWarning", "-c", code], capture_output=True, text=True, check=True, env=env
    )

    assert done.stdout.strip() == "False"
