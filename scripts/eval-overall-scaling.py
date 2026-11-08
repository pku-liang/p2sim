#!/usr/bin/env python3
"""
Experiment 1 & 2: Overall Performance and Thread Scaling Evaluation
Repository: git@github.com:pku-liang/p2sim.git
Branch: v2-exp
Base Commit: 890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1
Designs: Rocket (rocket_m_1), Small BOOM (boom_s_1), Large BOOM (boom_l_1)
Workloads: BootLinux (linux.bin), CoreMark (div-1-coremark.bin), Dhrystone (div-1-dhrystone.bin)
Simulators: S2SIM (spec-sim), P2SIM (baseline), GSIM, Verilator (VLT)
Threads: 1, 2, 4, 8, 16, 24 (pinned to physical cores on Socket 0/1)
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
FOUR_BACKENDS = ROOT / "build/scaling-four-backends-20261029"
MATRIX_SCALING = ROOT / "build/thread-scaling-20261105/matrix"
OUT_DIR = ROOT / "results/eval-overall-scaling-20261108"
BOOM_L_CYCLES = 2_000_000
THREADS = [1, 2, 4, 8, 16, 24]
DESIGNS = ["rocket_m_1", "boom_s_1", "boom_l_1"]
WORKLOADS = [
    ("linux", "linux.bin"),
    ("coremark", "div-1-coremark.bin"),
    ("dhry", "div-1-dhrystone.bin"),
]

spec = importlib.util.spec_from_file_location("audit", ROOT / "scripts/audit-spec-benchmarks.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def linux_lines(raw: str) -> list[str]:
    raw = audit.SAMPLE.sub("", raw).replace("\r", "")
    raw = re.split(r"start simulation: max_cycles=\d+\n", raw)[-1]
    lines = raw.splitlines()
    if not raw.endswith("\n"):
        lines = lines[:-1]
    return [re.sub(r"^\[\s*[0-9.]+\]\s*", "", line) for line in lines if line.strip()]


def measure_boom_l_baselines() -> None:
    builds = json.loads((FOUR_BACKENDS / "builds.json").read_text())
    good = {b["key"]: b for b in builds if b.get("status") == "ok"}
    records_path = FOUR_BACKENDS / "measurements.json"
    records = json.loads(records_path.read_text()) if records_path.exists() else []
    lock = Lock()

    def run_one(mode: str, n: int, bench: str, img_name: str, cpuset: str) -> None:
        key = f"boom_l_1/{mode}/T{n}"
        with lock:
            if any(r.get("key") == key and r.get("workload") == bench and r.get("valid") for r in records):
                print(f"REUSE {key} {bench}", flush=True)
                return
        b = good[key]
        image = FOUR_BACKENDS / "inputs" / img_name
        folder = FOUR_BACKENDS / "measurements" / bench / "boom_l_1" / f"T{n}" / mode
        folder.mkdir(parents=True, exist_ok=True)
        log = folder / "r1.out"
        env = {**os.environ, "MAX_CYCLES": str(BOOM_L_CYCLES), "SIM_THREADS": str(n)}
        env.pop("PROF_FILE", None)
        cmd = ["taskset", "-c", cpuset, "stdbuf", "-oL", "-eL", b["binary"], str(image)]
        print(f"RUN {key} {bench} cpuset={cpuset}", flush=True)
        t0 = time.time()
        load_before = os.getloadavg()
        with log.open("w") as f:
            res = subprocess.run(cmd, env=env, stdout=f, stderr=subprocess.STDOUT, timeout=1800)
        wall = time.time() - t0
        raw = log.read_text(errors="replace")
        if res.returncode != 0:
            raise RuntimeError(f"{key} {bench} exited {res.returncode}")
        samples = audit.SAMPLE.findall(raw)
        if not samples or int(samples[-1][0]) != BOOM_L_CYCLES:
            raise RuntimeError(f"{key} {bench} cycle mismatch")
        if bench == "linux":
            lines = linux_lines(raw)
            if not any("bbl loader" in x for x in lines):
                raise RuntimeError(f"{key} linux missing boot milestone")
            if re.search(r"Kernel panic|Oops:|BUG:|Assertion.*failed", raw):
                raise RuntimeError(f"{key} linux panic or assertion failure")
            khz = float(samples[-1][1])
        else:
            khz = audit.validate(raw, bench, BOOM_L_CYCLES)
        trial = {
            "key": key,
            "workload": bench,
            "repeat": 1,
            "binary_sha256": b["binary_sha256"],
            "image_sha256": sha256(image),
            "cycles": BOOM_L_CYCLES,
            "cpuset": cpuset,
            "log": str(log.relative_to(FOUR_BACKENDS)),
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
            records_path.write_text(json.dumps(records, indent=2) + "\n")
        print(f"PASS {key} {bench}: {khz:.3f} kHz ({wall:.1f}s)", flush=True)

    # Wave 1: Disjoint core allocations across Socket 0 (0..27) and Socket 1 (28..55)
    # Each task gets dedicated physical cores
    wave1 = [
        ("gsim", 1, "linux", "linux.bin", "28"),
        ("gsim", 1, "coremark", "div-1-coremark.bin", "29"),
        ("gsim", 1, "dhry", "div-1-dhrystone.bin", "30"),
        ("verilator", 1, "linux", "linux.bin", "31"),
        ("verilator", 1, "coremark", "div-1-coremark.bin", "32"),
        ("verilator", 1, "dhry", "div-1-dhrystone.bin", "33"),
        ("verilator", 2, "linux", "linux.bin", "34-35"),
        ("verilator", 2, "coremark", "div-1-coremark.bin", "36-37"),
        ("verilator", 2, "dhry", "div-1-dhrystone.bin", "38-39"),
        ("verilator", 4, "linux", "linux.bin", "40-43"),
        ("verilator", 4, "coremark", "div-1-coremark.bin", "44-47"),
        ("verilator", 4, "dhry", "div-1-dhrystone.bin", "48-51"),
        ("verilator", 8, "linux", "linux.bin", "0-7"),
        ("verilator", 8, "coremark", "div-1-coremark.bin", "8-15"),
        ("verilator", 8, "dhry", "div-1-dhrystone.bin", "16-23"),
    ]
    with ThreadPoolExecutor(max_workers=len(wave1)) as pool:
        futs = [pool.submit(run_one, *args) for args in wave1]
        for fut in futs:
            fut.result()

    # Wave 2: T16 and T24 runs
    wave2_batches = [
        [
            ("verilator", 16, "linux", "linux.bin", "28-43"),
            ("verilator", 16, "coremark", "div-1-coremark.bin", "0-15"),
        ],
        [
            ("verilator", 16, "dhry", "div-1-dhrystone.bin", "28-43"),
            ("verilator", 24, "linux", "linux.bin", "0-23"),
        ],
        [
            ("verilator", 24, "coremark", "div-1-coremark.bin", "28-51"),
            ("verilator", 24, "dhry", "div-1-dhrystone.bin", "0-23"),
        ],
    ]
    for batch in wave2_batches:
        with ThreadPoolExecutor(max_workers=len(batch)) as pool:
            futs = [pool.submit(run_one, *args) for args in batch]
            for fut in futs:
                fut.result()


def consolidate() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    records = json.loads((FOUR_BACKENDS / "measurements.json").read_text())
    matrix = json.loads((MATRIX_SCALING / "report.json").read_text())

    scaling_rows = []
    for design in DESIGNS:
        for bench, _ in WORKLOADS:
            for n in THREADS:
                for mode, sim_name in [("off", "P2SIM"), ("on", "S2SIM")]:
                    trials = [
                        t["khz"]
                        for t in matrix["trials"]
                        if t["design"] == design
                        and t["benchmark"] == bench
                        and t["threads"] == n
                        and t["mode"] == mode
                        and t["valid"]
                    ]
                    assert len(trials) == 3, f"Expected 3 trials for {design} {bench} T{n} {mode}"
                    scaling_rows.append({
                        "design": design,
                        "workload": bench,
                        "simulator": sim_name,
                        "threads": n,
                        "median_khz": round(statistics.median(trials), 3),
                        "min_khz": round(min(trials), 3),
                        "max_khz": round(max(trials), 3),
                        "repeats": len(trials),
                    })
            for mode, sim_name, thread_list in [("gsim", "GSIM", [1]), ("verilator", "Verilator", THREADS)]:
                for n in thread_list:
                    key = f"{design}/{mode}/T{n}"
                    trials = [
                        r["frequency_khz"]
                        for r in records
                        if r.get("key") == key and r.get("workload") == bench and r.get("valid")
                    ]
                    assert trials, f"Missing {key} {bench}"
                    scaling_rows.append({
                        "design": design,
                        "workload": bench,
                        "simulator": sim_name,
                        "threads": n,
                        "median_khz": round(statistics.median(trials), 3),
                        "min_khz": round(min(trials), 3),
                        "max_khz": round(max(trials), 3),
                        "repeats": len(trials),
                    })

    with (OUT_DIR / "scaling_all.csv").open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["design", "workload", "simulator", "threads", "median_khz", "min_khz", "max_khz", "repeats"])
        w.writeheader()
        w.writerows(scaling_rows)

    overall_rows = []
    for design in DESIGNS:
        for bench, _ in WORKLOADS:
            sub = [r for r in scaling_rows if r["design"] == design and r["workload"] == bench]
            best = {}
            t24 = {}
            for sim_name in ["S2SIM", "P2SIM", "GSIM", "Verilator"]:
                sim_rows = [r for r in sub if r["simulator"] == sim_name]
                peak_row = max(sim_rows, key=lambda r: r["median_khz"])
                best[sim_name] = (peak_row["median_khz"], peak_row["threads"])
                t24_rows = [r for r in sim_rows if r["threads"] == 24]
                if t24_rows:
                    t24[sim_name] = t24_rows[0]["median_khz"]
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
                "s2sim_t24_khz": t24["S2SIM"],
                "p2sim_t24_khz": t24["P2SIM"],
                "vlt_t24_khz": t24["Verilator"],
                "peak_speedup_over_p2sim": round(best["S2SIM"][0] / best["P2SIM"][0], 3),
                "t24_speedup_over_p2sim": round(t24["S2SIM"] / t24["P2SIM"], 3),
                "peak_speedup_over_gsim": round(best["S2SIM"][0] / best["GSIM"][0], 3),
                "peak_speedup_over_vlt": round(best["S2SIM"][0] / vlt_peak, 3),
            })

    with (OUT_DIR / "overall_table.csv").open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(overall_rows[0].keys()))
        w.writeheader()
        w.writerows(overall_rows)

    (OUT_DIR / "report.json").write_text(json.dumps({
        "branch": "v2-exp",
        "base_commit": "890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1",
        "overall": overall_rows,
        "scaling": scaling_rows,
    }, indent=2) + "\n")
    print("WROTE", OUT_DIR / "report.json", flush=True)


if __name__ == "__main__":
    measure_boom_l_baselines()
    consolidate()
