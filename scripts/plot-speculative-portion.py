#!/usr/bin/env python3
import argparse
import re
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from matplotlib.ticker import PercentFormatter


BLOCK_RE = re.compile(r"Speculative partition \d+: blocks=(\d+)/(\d+)")


def parse_design(value: str) -> tuple[str, Path]:
    try:
        design, path = value.split("=", 1)
    except ValueError as error:
        raise argparse.ArgumentTypeError("expected DESIGN=LOG") from error
    return design, Path(path)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("design_logs", type=parse_design, nargs="+")
    parser.add_argument("--csv", type=Path, required=True)
    parser.add_argument("--outputs", type=Path, nargs="+", required=True)
    args = parser.parse_args()

    rows = []
    for design, path in args.design_logs:
        pairs = [
            tuple(map(int, match.groups()))
            for match in BLOCK_RE.finditer(path.read_text(errors="replace"))
        ]
        if not pairs:
            raise SystemExit(f"no speculative block statistics in {path}")
        speculative = sum(value for value, _ in pairs)
        total = sum(value for _, value in pairs)
        for is_speculative, count in (
            (False, total - speculative),
            (True, speculative),
        ):
            rows.append(
                {
                    "design": design,
                    "is-speculative": is_speculative,
                    "count": count,
                    "total": total,
                    "portion": count / total,
                }
            )

    data = pd.DataFrame(rows)
    args.csv.parent.mkdir(parents=True, exist_ok=True)
    data.to_csv(args.csv, index=False)

    sns.set_theme(style="whitegrid", context="notebook")
    hue_order = [False, True]
    figure, axis = plt.subplots(figsize=(8.4, 4.8))
    sns.barplot(
        data=data,
        x="design",
        y="portion",
        hue="is-speculative",
        hue_order=hue_order,
        palette={False: "#A0A7B4", True: "#F58518"},
        ax=axis,
    )
    axis.set_xlabel("Design")
    axis.set_ylabel("Portion")
    axis.set_title("Speculative scheduling-unit portion (T24)", fontweight="bold")
    axis.set_ylim(0, 1.08)
    axis.yaxis.set_major_formatter(PercentFormatter(1.0))
    axis.legend(
        title="is-speculative",
        loc="center left",
        bbox_to_anchor=(1.01, 0.5),
    )

    design_order = list(dict.fromkeys(data["design"]))
    for container, is_speculative in zip(axis.containers, hue_order):
        subset = data[data["is-speculative"] == is_speculative].set_index("design")
        labels = [
            f"{subset.loc[design, 'portion']:.1%}\n"
            f"{int(subset.loc[design, 'count']):,}/{int(subset.loc[design, 'total']):,}"
            for design in design_order
        ]
        axis.bar_label(container, labels=labels, padding=3, fontsize=9)

    sns.despine()
    figure.tight_layout()
    for output in args.outputs:
        output.parent.mkdir(parents=True, exist_ok=True)
        figure.savefig(output, dpi=220, bbox_inches="tight")
        print(f"wrote {output}")
    plt.close(figure)


if __name__ == "__main__":
    main()
