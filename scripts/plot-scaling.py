#!/usr/bin/env python3
import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt
import pandas as pd
import seaborn as sns
from matplotlib.ticker import StrMethodFormatter


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("data", type=Path)
    parser.add_argument("outputs", type=Path, nargs="+")
    parser.add_argument("--title", default="Simulation scaling")
    args = parser.parse_args()

    data = pd.read_csv(args.data)
    required = {"benchmark", "simulator", "threads", "speed_hz"}
    missing = required - set(data.columns)
    if missing:
        raise SystemExit(f"missing columns: {', '.join(sorted(missing))}")

    sns.set_theme(style="whitegrid", context="notebook")
    hue_order = [name for name in ("p2sim", "spec-sim") if name in set(data.simulator)]
    col_order = [
        name for name in ("Dhrystone", "CoreMark") if name in set(data.benchmark)
    ]
    row = "design" if "design" in data.columns and data.design.nunique() > 1 else None
    row_order = sorted(data.design.unique()) if row else None
    plot = sns.relplot(
        data=data,
        kind="line",
        col="benchmark",
        col_order=col_order or None,
        row=row,
        row_order=row_order,
        hue="simulator",
        hue_order=hue_order or None,
        x="threads",
        y="speed_hz",
        marker="o",
        dashes=False,
        height=3.6,
        aspect=1.15,
        facet_kws={"sharey": False},
        palette={"p2sim": "#4C78A8", "spec-sim": "#F58518"},
    )
    plot.set_titles("{row_name} — {col_name}" if row else "{col_name}")
    plot.set_axis_labels("Threads", "Speed (Hz)")
    thread_ticks = sorted(data.threads.unique())
    for axis in plot.axes.flat:
        axis.set_xscale("log", base=2)
        axis.set_xticks(thread_ticks)
        axis.xaxis.set_major_formatter(StrMethodFormatter("{x:g}"))
        axis.minorticks_off()
        axis.yaxis.set_major_formatter(StrMethodFormatter("{x:,.0f}"))
        axis.grid(True, axis="both", alpha=0.3)
    if plot.legend is not None:
        plot.legend.set_title("Simulator")
    plot.figure.suptitle(args.title, y=1.02, fontweight="bold")

    for output in args.outputs:
        output.parent.mkdir(parents=True, exist_ok=True)
        plot.figure.savefig(output, dpi=220, bbox_inches="tight")
        print(f"wrote {output}")
    plt.close(plot.figure)


if __name__ == "__main__":
    main()
