# alai-core

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

A project depends on it from GitHub, pinned to a commit, in its
`[tool.uv.sources]`:

```toml
alai-core = { git = "https://github.com/alai-sh/alai-core", rev = "<commit>" }
```

To work on both at once, point it at a local checkout instead:

```toml
alai-core = { path = "../alai-core", editable = true }
```

## License

MIT, see [LICENSE](LICENSE).
