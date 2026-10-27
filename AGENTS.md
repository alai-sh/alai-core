# Agent notes

- A shared library for alai, alai-doc and alai-index, not a client. No
  settings, project discovery, caches or network.
- Logic goes in `src/rust/core/`, with no pyo3. The binding in `src/rust/`
  only converts.
- alai links this crate and imports this package in one process. Never
  pass native objects between `alai._native` and `alai_core._native`.
- Public modules are imported by name, as `alai_core.semver`, never
  through `__init__.py`, which imports nothing.
- Design the API for what it does. alai-cli changes to fit it.
- Keep `src/alai_core/_native.pyi` in step with the binding, and keep
  `gil_used = false` true.
- Core must build and pass on Windows, though alai doesn't run there.
- After switching `.venv` to another Python, delete
  `src/alai_core/_native*.so` (or `.pyd`).
- Comments are sparse, in short plain sentences, without `;` or `:`.
- Every `.py`, `.pyi` and `.rs` file starts with the two-line copyright and
  MIT header.
- Nothing is released, so no compatibility shims. Rename and replace
  outright.
- Checks are `uv run tox run -f sanity` and `uv run tox run -f py314` (any
  of `py310` to `py315`, `py314t`, `py315t`).
