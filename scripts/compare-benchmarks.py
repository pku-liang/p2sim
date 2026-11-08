#!/usr/bin/env python3
import argparse
import re
from pathlib import Path


FREQ_RE = re.compile(r"^cycle=(\d+),.*\bfreq=([0-9.]+) kHz$", re.MULTILINE)


def last_frequency(path: Path) -> tuple[int, float]:
    matches = FREQ_RE.findall(path.read_text(errors="replace"))
    if not matches:
        raise ValueError(f"no frequency samples in {path}")
    cycle, frequency = matches[-1]
    return int(cycle), float(frequency)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", type=Path)
    parser.add_argument("spec", type=Path)
    parser.add_argument("--minimum-percent", type=float, default=30.0)
    args = parser.parse_args()

    baseline_cycle, baseline = last_frequency(args.baseline)
    spec_cycle, spec = last_frequency(args.spec)
    if baseline_cycle != spec_cycle:
        raise SystemExit(
            f"cycle mismatch: baseline={baseline_cycle}, spec={spec_cycle}"
        )

    improvement = (spec / baseline - 1.0) * 100.0
    print(f"cycles={baseline_cycle}")
    print(f"baseline_khz={baseline:.3f}")
    print(f"spec_khz={spec:.3f}")
    print(f"improvement_percent={improvement:.2f}")
    if improvement < args.minimum_percent:
        raise SystemExit(
            f"improvement {improvement:.2f}% is below {args.minimum_percent:.2f}%"
        )


if __name__ == "__main__":
    main()
