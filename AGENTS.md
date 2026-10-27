# Agent notes

Rules the code and tooling don't show, and the decisions that shape the
project. Keep this short: record a decision only when getting it wrong
would be costly, and drop anything a tool now enforces.

## What alai-core is

- A library for the alai tools: alai (alai-cli), alai-doc and alai-index.
  It's not a client: settings, project discovery, caches, the network and
  presentation stay in alai, which other tools call as a subprocess.
- **It owns the codecs and the schemas tools share; each tool owns its
  own.** Codecs: TOML, strict JSON Lines, YAML as PyYAML reads it. Shared
  schemas: the index-spec formats and a collection's own metadata
  (`galaxy.yml`, `MANIFEST.json`, `FILES.json`, `meta/runtime.yml`).
  `alai.toml`, `alai.lock` and alai's cache layouts stay in alai.
- **Two crates**, both `publish = false`: `src/rust/core/` is `alai-core`,
  the logic in plain Rust with no pyo3; `src/rust/` is `alai-core-py`, the
  binding maturin builds as `alai_core._native`, which only converts:
  Python arguments into Rust values, results back, and errors into
  exceptions. Core functions take and return plain data (structs, slices
  of their input, `serde_json` values), never a Python object or a
  callback.
- alai imports this package, and its Rust may depend on the `alai-core`
  crate directly (a git dependency by `rev`). That puts two compiled
  copies of core in one process: never pass a native object between
  `alai._native` and `alai_core._native`, keep no global state both would
  need to see, and pin the crate and the package to the same `rev` or
  release.

## Rules

- Modules in `src/alai_core/` are private (`_name.py`) until a public API
  is designed for them; a public module re-exports what it promises.
  `__init__.py` imports nothing, so importing one part never loads another
  or the native extension.
- The binding is built into the package as `alai_core._native` by
  maturin, typed by `src/alai_core/_native.pyi`; keep the stub in step
  with it. The module declares `gil_used = false`, and must stay safe
  without the GIL. Building alai-core and its tests needs `cargo`. Its
  version is in the root `Cargo.toml` (`[workspace.package]`), which
  `pyproject.toml` takes through maturin. `uv sync` builds it into
  `src/alai_core/`, named for the Python's ABI, so after switching `.venv`
  to another Python, delete `src/alai_core/_native*.so` (`.pyd` on
  Windows; `tests/test_native.py` fails if more than one is there).
- **Linux, macOS and Windows.** alai doesn't run on Windows, but core
  mustn't rule out a tool that does, such as alai-index, so CI builds and
  tests it there too. What only works on Unix (file modes, symlinks,
  clones) goes behind `#[cfg(unix)]` or `os.name` with a Windows behaviour
  or a clear error, and paths are never assumed to use `/`.
- Every `.py`, `.pyi` and `.rs` file starts with the two-line copyright/MIT
  header; copy it from an existing file.
- Code moved from alai-cli keeps its behaviour, messages and Python tests.
  Its Rust is split into logic (`core/`) and conversion (the binding) as
  it moves; any other change comes in a later commit.
- Nothing is released, so there are no backwards-compatibility shims or
  aliases: rename and replace outright.
- Checks:
  - `uv run tox run -f sanity` (ruff, basedpyright, `cargo fmt`, clippy,
    and the `alai-core` crate's Rust tests)
  - `uv run tox run -f py314` (tests; any of `py310` to `py315`, and the
    free-threaded `py314t` and `py315t`)
