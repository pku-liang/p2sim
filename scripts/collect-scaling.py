#!/usr/bin/env python3
import argparse
import csv
import re
from pathlib import Path


THREAD_RE = re.compile(r"^T([1-9][0-9]*)$")
BENCHMARK_NAMES = {"dhry": "Dhrystone", "coremark": "CoreMark"}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("results", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--design")
    args = parser.parse_args()
    design = args.design or args.results.parent.name

    rows = []
    for summary in sorted(args.results.glob("T*/*/summary.tsv")):
        match = THREAD_RE.match(summary.parent.parent.name)
        if not match:
            continue
        threads = int(match.group(1))
        simulator = summary.parent.name
        with summary.open(newline="") as file:
            for record in csv.DictReader(file, delimiter="\t"):
                benchmark = BENCHMARK_NAMES.get(
                    record["benchmark"], record["benchmark"]
                )
                frequency_khz = float(record["frequency_khz"])
                rows.append(
                    {
                        "design": design,
                        "benchmark": benchmark,
                        "simulator": simulator,
                        "threads": threads,
                        "speed_hz": round(frequency_khz * 1000.0, 3),
                        "frequency_khz": frequency_khz,
                        "cycles": int(record["cycles"]),
                        "log": str(summary.parent / f"{record['benchmark']}.out"),
                    }
                )

    if not rows:
        raise SystemExit(f"no summary files found under {args.results}")

    rows.sort(key=lambda row: (row["benchmark"], row["simulator"], row["threads"]))
    keys = [
        "design",
        "benchmark",
        "simulator",
        "threads",
        "speed_hz",
        "frequency_khz",
        "cycles",
        "log",
    ]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=keys)
        writer.writeheader()
        writer.writerows(rows)

    print(f"wrote {len(rows)} rows to {args.output}")


if __name__ == "__main__":
    main()
