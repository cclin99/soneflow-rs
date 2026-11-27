"""Recompute both measurement sets from the retained Criterion payloads."""
import json
from pathlib import Path
import statistics

root = Path(__file__).resolve().parent
perf = root / "performance"
count = 0
for batch in (1, 2):
    data = json.loads((perf / f"batch{batch}-summary.json").read_text())
    for row in data["rows"]:
        raw = json.loads((perf / row["evidence"]).read_text())
        assert raw["median"]["point_estimate"] == row["median_ns"]
        count += 1
    for summary in data["summaries"]:
        values = [r["median_ns"] for r in data["rows"]
                  if (r["arm"], r["version"]) == (summary["arm"], summary["version"])]
        assert values == summary["medians_ns"]
        assert statistics.median(values) == summary["median_ns"]
        assert min(values) == summary["min_ns"]
        assert max(values) == summary["max_ns"]
        assert max(values) - min(values) == summary["range_ns"]
        assert summary["range_ns"] / summary["median_ns"] == summary["range_fraction"]
    assert data["obvious_noise"] == any(s["range_fraction"] > .05 for s in data["summaries"])

decision = json.loads((perf / "decision.json").read_text())
assert decision["selected_batch"] == 2
assert not decision["residual_obvious_noise"]
for arm in decision["decisions"]:
    base, new = [next(s for s in data["summaries"]
                     if s["arm"] == arm["arm"] and s["version"] == version)
                 for version in ("base", "new")]
    delta = new["median_ns"] - base["median_ns"]
    noise = max(base["range_ns"], new["range_ns"])
    assert arm["delta_pct"] == 100 * delta / base["median_ns"]
    assert arm["noise_bound_ns"] == noise
    assert arm["no_regression"] == (delta <= noise)
    assert arm["median_improves"] == (delta < 0)
result = {"retained_estimates_checked": count, "summary_or_decision_mismatches": 0,
          "selected_batch": 2}
(root / "timing-crosscheck.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result))
