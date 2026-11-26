# User-only checks / 交使用者執行

Run on fixed commit 0ed8adbb256ad29ba43781f57bd5ac370e549121 with your licensed local data and a wheel built from that commit. These commands were NOT executed by the agent. No ignored tests were enabled.

```powershell
# Run from the fixed-commit repository root.
cargo test -p soneflow --release --features internals --test golden_core --test golden_dsp --test golden_zwst --test golden_zwtv --test stream_golden --test annexb --test annexb_validator -- --nocapture
.venv\Scripts\python.exe -m pytest soneflow-py/tests/test_parity_mosqito.py -v
python private/tools/iso_material_check.py --check
```
