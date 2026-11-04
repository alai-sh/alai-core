# Agent notes

- A shared library for alai, alai-doc and alai-index, not a client. No
  settings, project discovery, caches or network.
- Logic goes in `src/rust/core/`, with no pyo3. The binding in `src/rust/`
  only converts.
- alai links this crate and imports this package in one process. Never
  pass native objects between `alai._native` and `alai_core._native`.
- Public modules are imported by name, as `alai_core.semver`, never
  through `__init__.py`, which imports nothing.
- Design the API for what it does. alai changes to fit it.
- Keep `src/alai_core/_native.pyi` in step with the binding, and keep
  `gil_used = false` true.
- Docstrings, the stub's included, are Google style (`Args:`, `Returns:`,
  `Raises:`, `Example:`). `tests/test_stub_examples.py` runs the stub's
  examples, so take their output from a real run.
- Core must build and pass on Windows, though alai doesn't run there.
- After switching `.venv` to another Python, delete
  `src/alai_core/_native*.so` (or `.pyd`).
- What users read is US English, `README.md`, `CHANGELOG.md` and error
  messages, without `;` or `:` in the text. An error can start with what
  it's about, as `invalid version '1.x': `, which callers join to the cause.
- How what users read is written:
  - Start with the situation the reader is in, then what happens or what to
    do. Describe an option by the problem it solves, such as "When syncing
    against a server with lots of collections and versions, `--limit N` can
    be used to perform the sync in smaller batches", not as a bare
    instruction.
  - Give each sentence one main idea. Join a second point to it with "and",
    "so", "as", "but", "when" or "which" only when it completes or explains
    the first, and start a new sentence for the next idea.
  - Keep a sentence to two commas or fewer. One that needs more is carrying
    more than one idea, so split it where the ideas change.
  - Don't write runs of very short sentences that only make sense together.
    When avoiding `;` or `:`, join the two halves with one of the words
    above rather than splitting them into separate sentences.
  - Don't push a phrase into the middle of a clause, between a subject and
    its verb or between "which" and what it says. A condition goes at the
    start or the end of a sentence.
  - Each sentence should make sense on its own, so name the thing rather
    than opening with "this", "that" or "it" pointing at the sentence before.
  - Don't drop words to shorten a phrase, such as "`sha256`, its artifact's"
    for "the artifact's sha256 checksum".
- Comments are sparse, in short plain sentences, without `;` or `:`.
- Every `.py`, `.pyi` and `.rs` file starts with the two-line copyright and
  MIT header.
- Releases before 0.1 can change the API, so no compatibility shims.
  Rename and replace outright, and list the change in `CHANGELOG.md`.
- The wheels ship `THIRD-PARTY-LICENSES` for the crates they link. After a
  change to `Cargo.lock`, write it again with
  `cargo about generate --locked about.hbs -o THIRD-PARTY-LICENSES`, which
  CI checks. A crate under a license `about.toml` doesn't accept fails the
  check, so look at the license before adding it.
- Checks are `uv run tox run -f sanity` and `uv run tox run -f py314` (any
  of `py310` to `py315`, `py314t`, `py315t`).
