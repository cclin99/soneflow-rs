# Release checklist

> 對照／Pair: [中文版](RELEASE-CHECKLIST.zh-TW.md) · source: en · synced 2026-11-28

Run this checklist immediately before publishing a release or making the README public.

- [ ] Confirm that neither `README.md` nor `README.zh-TW.md` carries review-draft wording; no public README may say "not yet announced as a public release" or "尚未對外宣布發布".
- [ ] Re-run `tools/compare_mosqito.py` and update the date, environment, timing, and direct-parity values together.
- [ ] Run the commands under "Measured results (0.2.1)" → "Reproduce" / 「實測結果（0.2.1）」→「重現」 from a locally regenerated golden environment.
- [ ] Confirm that `docs/GOLDEN-REGEN-SOP.md` still describes every prerequisite needed by a fresh clone.
- [ ] Check every capability and boundary statement against the tagged source and public artifacts.
- [ ] Review `THIRD_PARTY_NOTICES.md`, `CITATION.cff`, `SECURITY.md`, and community templates against the release tag and current repository settings.
- [ ] With the normal antivirus protection enabled, build and scan the release DLL/static library, wheel, and any plug-in bundle; record any detection by product, version, artifact hash, and exact build command. Do not treat a disabled-antivirus build as release evidence.
- [ ] Run `llvm-objdump -d -r` on every release `soneflow` rlib (`target/release/deps/` holds one `libsoneflow-*.rlib` per feature set that was built) and confirm that `init_mp1_table_rn` has 20 `pow` relocations in each (on Windows COFF they appear only with `-r`; Linux ELF uses other relocation types). `std::hint::black_box` is only best effort, so after a toolchain upgrade this confirms that the compiler has not folded these `pow` calls at compile time.
- [ ] Generate and publish SHA-256 checksums for every release artifact; verify downloaded artifacts against those checksums before smoke testing.
- [ ] Confirm that local-only files, archives, generated `data/`, credentials, and machine-specific paths are not staged.
- [ ] Run the maintainer's local check that no ISO 532-1 electronic-insert material (Annex A program, Annex B signals or reference values) is in the tree, and check by hand the formats it cannot see (rewritten code, renamed columns, binaries, archives).
- [ ] Audit and verification deliveries: run `python tools/redact_paths.py --check docs` (must print `clean`); every evidence archive a report refers to is listed in that delivery's own folder under `docs/verification/` with size, SHA-256 and a working download link; the archives themselves are not committed.
