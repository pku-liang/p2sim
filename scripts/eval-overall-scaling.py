#!/usr/bin/env python3
"""
Measure complete 4-backend (origin P2SIM, spec S2SIM, GSIM, Verilator) scaling
across [1, 2, 4, 8, 16, 24, 28, 32] threads for Rocket, Small BOOM, and Large BOOM
on BootLinux, CoreMark, and Dhrystone.
Strictly uses physical CPUs 24..55 to avoid interfering with Socket 0 (0..23).
Baseline is original P2SIM (origin-generator from commit 4029b74).
"""
from concurrent.futures import ThreadPoolExecutor
from threading import Lock
import csv
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time

ROOT = Path.home() / "pro/p2sim-next/p2sim"
OUT = ROOT / "build/scaling-four-backends-20261029"
OUT_RES = ROOT / "results/eval-overall-scaling-20261108"
CYCLES = 5_000_000
VLT_CYCLES = 500_000
THREADS = [1, 2, 4, 8, 16, 24, 28, 32]
DESIGNS = ["rocket_m_1", "boom_s_1", "boom_l_1"]
WORKLOADS = [
    ("linux", "linux.bin"),
    ("coremark", "div-1-coremark.bin"),
    ("dhry", "div-1-dhrystone.bin"),
]

spec = importlib.util.spec_from_file_location("audit", OUT / "audit-spec-benchmarks.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def linux_lines(raw: str) -> list[str]:
    raw = audit.SAMPLE.sub("", raw).replace("\r", "")
    raw = re.split(r"start simulation: max_cycles=\d+\n", raw)[-1]
    lines = raw.splitlines()
    if not raw.endswith("\n"):
        lines = lines[:-1]
    return [re.sub(r"^\[\s*[0-9.]+\]\s*", "", line) for line in lines if line.strip()]


def main() -> None:
    os.chdir(ROOT)
    builds = json.loads((OUT / "builds.json").read_text())
    good = {b["key"]: b for b in builds if b.get("status") == "ok"}
    rec_path = OUT / "measurements.json"
    records = json.loads(rec_path.read_text()) if rec_path.exists() else []
    lock = Lock()

    def save():
        rec_path.write_text(json.dumps(records, indent=2) + "\n")

    def run_one(design: str, mode: str, n: int, bench: str, img_name: str, cpuset: str) -> None:
        key = f"{design}/{mode}/T{n}"
        with lock:
            if any(r.get("key") == key and r.get("workload") == bench and r.get("valid") for r in records):
                print(f"REUSE {key} {bench}", flush=True)
                return
        b = good[key]
        is_vlt = mode == "verilator"
        cycles = VLT_CYCLES if is_vlt else CYCLES
        image = OUT / "inputs" / img_name
        folder = OUT / "measurements" / bench / design / f"T{n}" / mode
        folder.mkdir(parents=True, exist_ok=True)
        log = folder / "r1.out"
        env = {**os.environ, "MAX_CYCLES": str(cycles), "SIM_THREADS": str(n)}
        env.pop("PROF_FILE", None)
        cmd = ["taskset", "-c", cpuset, "stdbuf", "-oL", "-eL", b["binary"], str(image)]
        print(f"RUN {key} {bench} (cpuset={cpuset})", flush=True)
        t0 = time.time()
        load_before = os.getloadavg()
        with log.open("w") as f:
            res = subprocess.run(cmd, env=env, stdout=f, stderr=subprocess.STDOUT, timeout=1800)
        wall = time.time() - t0
        raw = log.read_text(errors="replace")
        if res.returncode != 0:
            raise RuntimeError(f"{key} {bench} exited {res.returncode}")
        samples = audit.SAMPLE.findall(raw)
        if not samples or int(samples[-1][0]) != cycles:
            raise RuntimeError(f"{key} {bench} cycle mismatch")
        if is_vlt:
            khz = float(samples[-1][1])
        elif bench == "linux":
            lines = linux_lines(raw)
            if not any("Linux version 4.18" in x for x in lines) or not any("bbl loader" in x for x in lines):
                raise RuntimeError(f"{key} linux missing boot milestones")
            if re.search(r"Kernel panic|Oops:|BUG:|Assertion.*failed", raw):
                raise RuntimeError(f"{key} linux panic/assert failure")
            khz = float(samples[-1][1])
        else:
            khz = audit.validate(raw, bench, cycles)

        trial = {
            "key": key,
            "workload": bench,
            "repeat": 1,
            "binary_sha256": b["binary_sha256"],
            "image_sha256": sha256(image),
            "cycles": cycles,
            "cpuset": cpuset,
            "log": str(log.relative_to(OUT)),
            "log_sha256": sha256(log),
            "started": t0,
            "wall_seconds": wall,
            "load_before": load_before,
            "load_after": os.getloadavg(),
            "returncode": res.returncode,
            "frequency_khz": khz,
            "valid": True,
        }
        with lock:
            records.append(trial)
            save()
        print(f"PASS {key} {bench}: {khz:.3f} kHz ({wall:.1f}s)", flush=True)

    def run_pair(design: str, n: int, bench: str, img_name: str, cpuset: str, include_vlt: bool = False) -> None:
        run_one(design, "origin", n, bench, img_name, cpuset)
        run_one(design, "spec", n, bench, img_name, cpuset)
        if include_vlt:
            run_one(design, "verilator", n, bench, img_name, cpuset)

    # Wave 1: Remaining boom_l_1 T1 and T8 on cores 28..45 (all inside 24..55)
    wave_1 = [
        ("boom_l_1", 1, "coremark", "div-1-coremark.bin", "28", False),
        ("boom_l_1", 1, "dhry", "div-1-dhrystone.bin", "29", False),
        ("boom_l_1", 8, "linux", "linux.bin", "30-37", False),
        ("boom_l_1", 8, "coremark", "div-1-coremark.bin", "38-45", False),
    ]
    with ThreadPoolExecutor(max_workers=len(wave_1)) as pool:
        for fut in [pool.submit(run_pair, *args) for args in wave_1]:
            fut.result()

    # Wave 2: boom_l_1 T16 on 24-39 and 40-55
    wave_2_batches = [
        [
            ("boom_l_1", 16, "linux", "linux.bin", "28-43", False),
            ("boom_l_1", 16, "coremark", "div-1-coremark.bin", "40-55", False),
        ],
        [
            ("boom_l_1", 16, "dhry", "div-1-dhrystone.bin", "28-43", False),
        ],
    ]
    for batch in wave_2_batches:
        with ThreadPoolExecutor(max_workers=len(batch)) as pool:
            for fut in [pool.submit(run_pair, *args) for args in batch]:
                fut.result()

    # Wave 3: boom_l_1 T24 (28-51), then all T28 (28-55), then all T32 (24-55)
    for bench, img in WORKLOADS:
        run_pair("boom_l_1", 24, bench, img, "28-51", False)
    for d in DESIGNS:
        for bench, img in WORKLOADS:
            run_pair(d, 28, bench, img, "28-55", True)
    for d in DESIGNS:
        for bench, img in WORKLOADS:
            run_pair(d, 32, bench, img, "24-55", True)

    # Export consolidated CSVs and JSON
    OUT_RES.mkdir(parents=True, exist_ok=True)
    scaling_rows = []
    for design in DESIGNS:
        for bench, _ in WORKLOADS:
            for mode, sim_name, thread_list in [
                ("origin", "P2SIM", THREADS),
                ("spec", "S2SIM", THREADS),
                ("gsim", "GSIM", [1]),
                ("verilator", "Verilator", THREADS),
            ]:
                for n in thread_list:
                    key = f"{design}/{mode}/T{n}"
                    vals = [
                        r["frequency_khz"]
                        for r in records
                        if r.get("key") == key and r.get("workload") == bench and r.get("valid")
                    ]
                    assert vals, f"Missing measurement for {key} {bench}"
                    scaling_rows.append({
                        "design": design,
                        "workload": bench,
                        "simulator": sim_name,
                        "threads": n,
                        "median_khz": round(statistics.median(vals), 3),
                        "min_khz": round(min(vals), 3),
                        "max_khz": round(max(vals), 3),
                        "repeats": len(vals),
                    })

    with (OUT_RES / "scaling_all.csv").open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["design", "workload", "simulator", "threads", "median_khz", "min_khz", "max_khz", "repeats"])
        w.writeheader()
        w.writerows(scaling_rows)

    overall_rows = []
    for design in DESIGNS:
        for bench, _ in WORKLOADS:
            sub = [r for r in scaling_rows if r["design"] == design and r["workload"] == bench]
            best = {}
            by_t = {sim: {} for sim in ["S2SIM", "P2SIM", "GSIM", "Verilator"]}
            for sim_name in ["S2SIM", "P2SIM", "GSIM", "Verilator"]:
                sim_rows = [r for r in sub if r["simulator"] == sim_name]
                peak_row = max(sim_rows, key=lambda r: r["median_khz"])
                best[sim_name] = (peak_row["median_khz"], peak_row["threads"])
                for r in sim_rows:
                    by_t[sim_name][r["threads"]] = r["median_khz"]
            vlt_peak = best["Verilator"][0]
            overall_rows.append({
                "design": design,
                "workload": bench,
                "s2sim_peak_khz": best["S2SIM"][0],
                "s2sim_peak_threads": best["S2SIM"][1],
                "p2sim_peak_khz": best["P2SIM"][0],
                "p2sim_peak_threads": best["P2SIM"][1],
                "gsim_khz": best["GSIM"][0],
                "vlt_peak_khz": best["Verilator"][0],
                "vlt_peak_threads": best["Verilator"][1],
                "s2sim_t24_khz": by_t["S2SIM"][24],
                "p2sim_t24_khz": by_t["P2SIM"][24],
                "s2sim_t28_khz": by_t["S2SIM"][28],
                "p2sim_t28_khz": by_t["P2SIM"][28],
                "s2sim_t32_khz": by_t["S2SIM"][32],
                "p2sim_t32_khz": by_t["P2SIM"][32],
                "vlt_t24_khz": by_t["Verilator"][24],
                "vlt_t28_khz": by_t["Verilator"][28],
                "vlt_t32_khz": by_t["Verilator"][32],
                "peak_speedup_over_p2sim": round(best["S2SIM"][0] / best["P2SIM"][0], 3),
                "t24_speedup_over_p2sim": round(by_t["S2SIM"][24] / by_t["P2SIM"][24], 3),
                "t28_speedup_over_p2sim": round(by_t["S2SIM"][28] / by_t["P2SIM"][28], 3),
                "t32_speedup_over_p2sim": round(by_t["S2SIM"][32] / by_t["P2SIM"][32], 3),
                "peak_speedup_over_gsim": round(best["S2SIM"][0] / best["GSIM"][0], 3),
                "peak_speedup_over_vlt": round(best["S2SIM"][0] / vlt_peak, 3),
            })

    with (OUT_RES / "overall_table.csv").open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(overall_rows[0].keys()))
        w.writeheader()
        w.writerows(overall_rows)

    (OUT_RES / "report.json").write_text(json.dumps({
        "branch": "v2-exp",
        "baseline_origin_commit": "4029b74",
        "s2sim_commit": "890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1",
        "threads": THREADS,
        "overall": overall_rows,
        "scaling": scaling_rows,
    }, indent=2) + "\n")
    print("WROTE", OUT_RES / "report.json", flush=True)


if __name__ == "__main__":
    main()
