"""Reproduce the README's 3-second soneflow/MoSQITo comparison."""

import argparse
import platform
import statistics
import sys
import time
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parent))


FS = 48_000


def measured_runs(function, signal, iterations):
    function(signal, float(FS))
    durations = []
    result = None
    for _ in range(iterations):
        start = time.perf_counter()
        result = function(signal, float(FS))
        durations.append(time.perf_counter() - start)
    return durations, result


def print_timing(label, durations, duration_s):
    mean_s = statistics.mean(durations)
    stdev_s = statistics.stdev(durations) if len(durations) > 1 else 0.0
    print(
        f"{label}: mean={mean_s:.6f} s, sample_sd={stdev_s:.6f} s, "
        f"min={min(durations):.6f} s, max={max(durations):.6f} s, "
        f"real_time={duration_s / mean_s:.4f}x"
    )
    return mean_s


def check_tables():
    """Check the three product arrays without exporting their values."""
    import ast
    import importlib
    import importlib.metadata
    import inspect
    import json
    import re
    try:
        import numpy as numpy
        version = importlib.metadata.version("mosqito")
        module = importlib.import_module("mosqito.sq_metrics.loudness.loudness_zwst._calc_slopes")
    except (ImportError, importlib.metadata.PackageNotFoundError):
        print(json.dumps({"status": "SKIPPED", "reason": "MoSQITo/NumPy unavailable"}))
        return 0
    if version != "1.2.1":
        print(json.dumps({"status": "FAIL", "reason": "expected MoSQITo 1.2.1", "actual": version}))
        return 1
    parsed = ast.parse(inspect.getsource(module))
    reference = {}
    for node in ast.walk(parsed):
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            name = node.targets[0].id
            if name in {"zup", "rns", "usl"} and isinstance(node.value, ast.Call):
                reference[name.upper()] = numpy.asarray(ast.literal_eval(node.value.args[0]), dtype=numpy.float64)
    source = (Path(__file__).resolve().parents[1] / "soneflow/src/tables.rs").read_text(encoding="utf-8")
    rows = []
    for name in ("ZUP", "RNS", "USL"):
        match = re.search(r"pub const " + name + r":.*?=\s*(\[.*?\]);", source, re.S)
        if match is None or name not in reference:
            raise RuntimeError(f"missing named table {name}")
        own = numpy.asarray(ast.literal_eval(match.group(1)), dtype=numpy.float64)
        other = reference[name]
        same_shape = own.shape == other.shape
        equal = same_shape and numpy.array_equal(own, other)
        rows.append({"table": name, "shape": list(own.shape), "entries": own.size,
                     "equal": bool(equal), "different_entries": int(numpy.count_nonzero(own != other)) if same_shape else None})
    ok = all(row["equal"] for row in rows)
    print(json.dumps({"status": "PASS" if ok else "FAIL", "mosqito_version": version, "tables": rows}))
    return 0 if ok else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iterations", type=int, default=30)
    parser.add_argument(
        "--nl-init",
        choices=("zero", "mosqito_tail"),
        default="zero",
        help="batch nonlinear-decay initialisation passed to soneflow.loudness_zwtv "
        "(zero: default; mosqito_tail: MoSQITo 1.2.1 seed)",
    )
    parser.add_argument("--check-tables", action="store_true", help="compare the three product tables with MoSQITo 1.2.1; report SKIPPED if unavailable")
    args = parser.parse_args()
    if args.check_tables:
        raise SystemExit(check_tables())
    global np
    import numpy as np
    from mosqito.sq_metrics import loudness_zwtv as mosqito_loudness_zwtv
    from soneflow_testkit import contract_signal
    import soneflow
    if args.iterations < 1:
        parser.error("--iterations must be at least 1")

    duration_s = 3
    signal = np.ascontiguousarray(contract_signal(duration_s * FS), dtype=np.float64)
    def rust_zwtv(x, fs):
        return soneflow.loudness_zwtv(x, fs, nl_init=args.nl_init)

    rust_runs, rust = measured_runs(rust_zwtv, signal, args.iterations)
    mosqito_runs, reference = measured_runs(
        mosqito_loudness_zwtv, signal, args.iterations
    )

    rust_n, rust_specific = rust[:2]
    reference_n, reference_specific = reference[:2]
    max_n = float(np.max(np.abs(rust_n - np.asarray(reference_n))))
    max_specific = float(
        np.max(np.abs(rust_specific - np.asarray(reference_specific)))
    )

    print(f"platform: {platform.platform()}")
    print(f"python: {platform.python_version()}; numpy: {np.__version__}")
    print(f"signal: contract_signal({duration_s * FS}), float64 contiguous, {FS} Hz")
    print(f"iterations per implementation: {args.iterations} (after one warm-up)")
    print(f"soneflow nl_init: {args.nl_init}")
    rust_s = print_timing("soneflow Rust binding", rust_runs, duration_s)
    mosqito_s = print_timing("MoSQITo", mosqito_runs, duration_s)
    print(f"relative speed from arithmetic means: {mosqito_s / rust_s:.2f}x")
    print(f"max abs N(t) difference: {max_n:.6e} sone")
    print(
        "max abs N_specific(t) difference: "
        f"{max_specific:.6e} sone/Bark"
    )


if __name__ == "__main__":
    main()
