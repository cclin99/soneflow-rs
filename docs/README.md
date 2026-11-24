# Documentation index

> 對照／Pair: [中文版](README.zh-TW.md) · source: zh · synced 2026-11-01

Language rule: `docs/DOCUMENTATION-LANGUAGE-POLICY.md`. Paired documents exist as `NAME.md` (English) and `NAME.zh-TW.md` (Traditional Chinese) with identical structure and numbers. `python tools/doc_pairs.py --check` verifies the pairs; `--list` prints the live status. Dates below are the `synced` date of the pair.

## Start here

| document | what it is | en | zh |
|---|---|---|---|
| [`../README.md`](../README.md) | project README: scope, install, API, verification scope | yes | [yes](../README.zh-TW.md) |
| [`ROADMAP.md`](ROADMAP.md) | roadmap and deferred items | yes | [yes](ROADMAP.zh-TW.md) |
| [`RELEASE-CHECKLIST.md`](RELEASE-CHECKLIST.md) | steps for every release | yes | [yes](RELEASE-CHECKLIST.zh-TW.md) |
| [`GOLDEN-REGEN-SOP.md`](GOLDEN-REGEN-SOP.md) | how the MoSQITo golden data is regenerated and verified | yes | [yes](GOLDEN-REGEN-SOP.zh-TW.md) |
| [`DOCUMENTATION-LANGUAGE-POLICY.md`](DOCUMENTATION-LANGUAGE-POLICY.md) | this language policy, glossary, writing principles | yes | [yes](DOCUMENTATION-LANGUAGE-POLICY.zh-TW.md) |

## Release notes

Changes per release are in [`../CHANGELOG.md`](../CHANGELOG.md).

## Independent verification

Deliveries go under `verification/<date>-<topic>-<commit>/` in the language they were written in; paths are redacted with `tools/redact_paths.py` before they are committed.
