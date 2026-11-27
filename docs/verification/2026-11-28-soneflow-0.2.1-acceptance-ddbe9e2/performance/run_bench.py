"""Independent acceptance orchestration; only synthetic loudness benchmarks."""
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent
ARMS = ["zwtv_10s_t1/scalar/480000", "zwtv_10s_t1/avx2/480000",
        "stream_push_10s_480chunk/scalar", "stream_push_10s_480chunk/avx2"]
ENV = dict(os.environ, TEMP=str(ROOT / "tmp"), TMP=str(ROOT / "tmp"),
           CARGO_HOME=str(ROOT / "cargo-home"), PYTHONDONTWRITEBYTECODE="1",
           RAYON_NUM_THREADS="1")

def measure(batch):
    rows = []
    for round_no in range(1, 4):
        for version in ("base", "new"):
            target = ROOT / f"perf-{version}-target"
            env = dict(ENV, CARGO_TARGET_DIR=str(target))
            label = f"batch{batch}-{round_no}-{version}"
            cmd = ["cargo", "bench", "--bench", "loudness", "--", "--noplot",
                   "zwtv_10s_t1|stream_push_10s_480chunk"]
            start = time.time()
            with (OUT / f"{label}.log").open("w", encoding="utf-8") as log:
                log.write(f"command={cmd!r}\nRAYON_NUM_THREADS=1\n")
                log.flush()
                result = subprocess.run(cmd, cwd=ROOT / f"perf-{version}" / "soneflow",
                                        env=env, stdout=log, stderr=subprocess.STDOUT)
            if result.returncode:
                raise RuntimeError(f"{label} exited {result.returncode}")
            for arm in ARMS:
                source = target / "criterion" / arm / "new" / "estimates.json"
                raw = source.read_text(encoding="utf-8")
                name = f"{label}-{arm.replace('/', '_')}-estimates.json"
                (OUT / name).write_text(raw, encoding="utf-8")
                median = json.loads(raw)["median"]["point_estimate"]
                rows.append(dict(batch=batch, round=round_no, version=version,
                                 arm=arm, median_ns=median, evidence=name))
            print(f"{label} completed in {time.time()-start:.1f}s", flush=True)
    summaries = []
    for arm in ARMS:
        for version in ("base", "new"):
            values = [r["median_ns"] for r in rows if r["arm"] == arm and r["version"] == version]
            med = statistics.median(values)
            summaries.append(dict(arm=arm, version=version, medians_ns=values,
                                  median_ns=med, min_ns=min(values), max_ns=max(values),
                                  range_ns=max(values)-min(values),
                                  range_fraction=(max(values)-min(values))/med))
    result = dict(rows=rows, summaries=summaries,
                  obvious_noise=any(s["range_fraction"] > .05 for s in summaries))
    (OUT / f"batch{batch}-summary.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    return result

if __name__ == "__main__":
    first = measure(1)
    chosen = measure(2) if first["obvious_noise"] else first
    decisions = []
    for arm in ARMS:
        base, new = [next(s for s in chosen["summaries"] if s["arm"] == arm and s["version"] == v)
                     for v in ("base", "new")]
        delta = new["median_ns"] - base["median_ns"]
        noise = max(base["range_ns"], new["range_ns"])
        decisions.append(dict(arm=arm, delta_pct=100*delta/base["median_ns"],
                              no_regression=delta <= noise,
                              median_improves=delta < 0, noise_bound_ns=noise))
    (OUT / "decision.json").write_text(json.dumps(dict(
        selected_batch=2 if first["obvious_noise"] else 1,
        residual_obvious_noise=chosen["obvious_noise"], decisions=decisions), indent=2), encoding="utf-8")
