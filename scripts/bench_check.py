# /// script
# requires-python = ">=3.12"
# ///
"""Gate startup latency from a hyperfine JSON export.

Budgets enforce; regressions inform. A path over its budget fails the run
(doubled when CI is set, because runners are slower and noisier). A path more
than --regression-pct slower than the committed baseline for this machine
class prints a warning and a table, never a failure. See
docs/reference/development.md.

    just bench-startup           measure, check, print the table
    just bench-startup --bless   rewrite the baseline for this machine class
"""

import argparse
import json
import os
import platform
import shutil
import sys
from pathlib import Path

BUDGETS_MS = {
    "rust": {"version": 10, "help": 20},
    "typescript": {"version": 20, "help": 30},
    "python": {"version": 40, "help": 60},
}
NOISE_FLOOR_MS = 2


def machine_class() -> str:
    return f"{platform.system().lower()}-{platform.machine().lower()}"


def path_kind(command: str) -> str:
    return "version" if command.rstrip().endswith(("--version", "-V")) else "help"


def load(path: Path) -> dict[str, float]:
    results = json.loads(path.read_text())["results"]
    return {r["command"]: r["mean"] * 1000 for r in results}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results", type=Path, help="hyperfine --export-json output")
    parser.add_argument("--baseline", type=Path, help="baseline JSON; default docs/bench/baseline-<os>-<arch>.json")
    parser.add_argument("--lang", choices=BUDGETS_MS, default="rust")
    parser.add_argument("--regression-pct", type=float, default=25.0)
    parser.add_argument("--bless", action="store_true", help="copy the results over the baseline")
    args = parser.parse_args()

    baseline_path = args.baseline or Path("docs/bench") / f"baseline-{machine_class()}.json"
    if args.bless:
        baseline_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(args.results, baseline_path)
        print(f"baseline written: {baseline_path}")
        return 0

    in_ci = bool(os.environ.get("CI"))
    factor = 2 if in_ci else 1
    budgets = {k: v * factor for k, v in BUDGETS_MS[args.lang].items()}
    current = load(args.results)
    baseline = load(baseline_path) if baseline_path.exists() else {}

    rows = []
    breached = []
    regressed = []
    for command, mean in current.items():
        kind = path_kind(command)
        budget = budgets[kind]
        base = baseline.get(command)
        delta = "" if base is None else f"{(mean - base) / base * 100:+.0f}%"
        status = "ok"
        if mean > budget:
            status = "OVER BUDGET"
            breached.append((command, mean, budget))
        elif base is not None and mean - base > NOISE_FLOOR_MS and (mean - base) / base * 100 > args.regression_pct:
            status = "regressed"
            regressed.append((command, mean, base))
        rows.append((command, f"{mean:.1f}", str(budget), f"{base:.1f}" if base is not None else "none", delta, status))

    table = ["| command | ms | budget | baseline | delta | status |", "|---|---|---|---|---|---|"]
    table += ["| " + " | ".join(row) + " |" for row in rows]
    text = "\n".join(table)
    print(text)
    if not baseline:
        print(f"no baseline for {machine_class()} at {baseline_path}; run `just bench-startup --bless` on this machine class to start one")

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write(f"## Startup bench ({machine_class()}, budgets x{factor})\n\n{text}\n\n")

    for command, mean, base in regressed:
        msg = f"{command}: {mean:.1f} ms, was {base:.1f} ms in the baseline"
        print(f"::warning title=startup regression::{msg}" if in_ci else f"warning: {msg}")
    for command, mean, budget in breached:
        msg = f"{command}: {mean:.1f} ms over the {budget} ms budget"
        print(f"::error title=startup budget::{msg}" if in_ci else f"error: {msg}", file=sys.stderr)
    return 1 if breached else 0


if __name__ == "__main__":
    sys.exit(main())
