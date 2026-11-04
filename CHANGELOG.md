# Changelog

All notable changes to alai-core are listed here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and alai-core
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Releases
before 0.1 can change their API in any release.

## [Unreleased]

### Added

- The wheels include `THIRD-PARTY-LICENSES`, which has the licenses of the
  Rust crates they're built from.

### Changed

- An index template with `{sha256-prefix}` but no `{sha256-checksum}` is
  now refused. A template finds files by their SHA-256 checksum only when it
  has `{sha256-checksum}`.

## [0.0.1] - 2026-11-03

### Added

- Reading and writing the alai index format's `config.json`,
  `collections.json`, index files and plugin index.
- SemVer versions and requirements, including ansible-galaxy's requirement
  syntax, and PEP 440 versions and specifiers.
- TOML and YAML helpers.
- Reading a collection's `MANIFEST.json`, `FILES.json` and
  `meta/runtime.yml`.
- Extracting a collection artifact.
- Static extraction of a collection's plugins, roles, playbooks and
  Event-Driven Ansible content with their documentation and routing.
- Static extraction of ansible-core's own plugins.
- A parser for Ansible's documentation markup.

[Unreleased]: https://github.com/alai-sh/alai-core/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/alai-sh/alai-core/releases/tag/v0.0.1
