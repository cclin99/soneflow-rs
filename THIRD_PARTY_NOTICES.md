# Third-party notices

soneflow is licensed under the Apache License 2.0. See [LICENSE](LICENSE).

## MoSQITo

This project uses MoSQITo 1.2.1 as a reference implementation for the ISO
532-1 processing flow, constants, boundary behavior, and locally generated
comparison fixtures.

- Project: MoSQITo
- Upstream: https://github.com/Eomys/MoSQITo
- Version used as the reference baseline: 1.2.1
- License: Apache License 2.0
- Copyright and authorship: retained by the upstream project and its contributors

soneflow is a native Rust reimplementation that uses MoSQITo 1.2.1 as its
reference. It is not part of, affiliated with, or endorsed by MoSQITo, Eomys, or
ISO. The upstream MoSQITo source archive and generated comparison data are local
development inputs and are not included in this repository.

The ISO standard text and ISO Annex B source material remain subject to their
respective rights. This repository's Apache-2.0 license does not grant rights to
redistribute ISO publications or test material.

The electronic insert of ISO 532-1:2017 (the Annex A program and the Annex B
test signals and workbooks) can be downloaded from ISO, whose licence for
electronic inserts permits use "in their original format without any
modifications for the purposes specified in their respective ISO standard(s)".
This repository therefore contains none of it: no source, no excerpts, no
patches carrying its lines, no test signals and no reference series. The
Annex B tests read their files from `data/annexb/`, which you fill from your own
licensed copy; `tools/golden.sha256` lists the hashes of the files the tests
were built with. CI does not run those tests.

## Rust and Python dependencies

The dependency graph is declared by `Cargo.toml`, `Cargo.lock`, and
`soneflow-py/pyproject.toml`. Each dependency remains under its own license and
copyright notices. Before redistributing binaries or wheels, review the exact
resolved dependency set and include any notices required by those licenses.
