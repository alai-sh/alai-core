# alai-core

[![PyPI](https://img.shields.io/pypi/v/alai-core)](https://pypi.org/project/alai-core/)
[![CI](https://github.com/alai-sh/alai-core/actions/workflows/ci.yml/badge.svg)](https://github.com/alai-sh/alai-core/actions/workflows/ci.yml)

> **Warning:** alai-core is for the alai tools' own use. It isn't
> designed for public consumption. Its API isn't stable and can change in
> any release, without notice.

alai-core holds what the alai tools share.

- the alai index format
- plugin indexes
- Ansible collection metadata
- plugin documentation extraction
- serialization formats (TOML, YAML, JSON)

alai-core is a Python package with a native extension in Rust, and
[alai](https://github.com/alai-sh/alai) uses it.

## Developing

Building alai-core needs `cargo` as well as [uv](https://docs.astral.sh/uv/).

```console
$ uv sync
$ uv run tox run -f sanity
$ uv run tox run -f py314
```

As the API isn't stable, a project that depends on alai-core from PyPI should
pin the exact version.

```toml
dependencies = ["alai-core == 0.1.0"]
```

## License

alai-core is MIT licensed, and the terms are in [LICENSE](LICENSE).
