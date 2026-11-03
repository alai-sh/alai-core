# alai-core

[![PyPI](https://img.shields.io/pypi/v/alai-core)](https://pypi.org/project/alai-core/)
[![CI](https://github.com/alai-sh/alai-core/actions/workflows/ci.yml/badge.svg)](https://github.com/alai-sh/alai-core/actions/workflows/ci.yml)

> **Warning:** alai-core is for the alai tools' own use. It isn't
> designed for public consumption. Its API isn't stable and can change in
> any release, without notice.

What the alai tools share:

- the alai index format
- plugin indexes
- Ansible collection metadata
- plugin documentation extraction
- serialization formats (TOML, YAML, JSON)

It's a Python package with a native extension in Rust, used by
[alai](https://github.com/alai-sh/alai-cli).

## Developing

Building it needs `cargo` as well as [uv](https://docs.astral.sh/uv/):

```console
$ uv sync
$ uv run tox run -f sanity
$ uv run tox run -f py314
```

A project depends on it from PyPI, as the API is not considered stable, you
should pin the exact version:

```toml
dependencies = ["alai-core == 0.0.1"]
```

When targeting a local copy with changes to `alai-core`, point the other
project's `[tool.uv.sources]` to the local checkout:

```toml
alai-core = { path = "../alai-core", editable = true }
```

## License

MIT, see [LICENSE](LICENSE).
