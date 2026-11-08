#!/usr/bin/env python3
"""
Experiment 3: Thread Execution Breakdown, Water-Flow Cycle Trace, and Straggler Analysis
Repository: git@github.com:pku-liang/p2sim.git
Branch: v2-exp
Base Commit: 890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1
Designs: Rocket (rocket_m_1), Small BOOM (boom_s_1), Large BOOM (boom_l_1)
Workloads: BootLinux (linux.bin), CoreMark (div-1-coremark.bin), Dhrystone (div-1-dhrystone.bin)
Configuration: 24 worker threads pinned to physical cores 28-51, 5,000,000 cycles
Metrics:
  1. Sampled cycle water-flow trace (barh separating Compute vs. Sync & State Update)
  2. Top-2 straggler threads time breakdown comparison (P2SIM vs. S2SIM)
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import numpy as np

ROOT = Path.home() / "pro/p2sim-next/p2sim"
BASE = ROOT / "build/spec-matrix-20261104"
WORKCOMM = ROOT / "build/boom-breakdown-20261105/phase-work-comm-v2"
OUT_DIR = ROOT / "results/eval-breakdown-20261108"
CYCLES = 5_000_000

spec = importlib.util.spec_from_file_location("audit", ROOT / "scripts/audit-spec-benchmarks.py")
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_instrumented(source: Path, dest: Path, mode: str) -> dict:
    dest.mkdir(parents=True, exist_ok=True)
    if (dest / "sim").exists() and (dest / "build.json").exists():
        return json.loads((dest / "build.json").read_text())
    mains = [
        p for p in (source / "src").glob("*.cc")
        if not p.name.startswith(("thread_", "blk_", "rst_", "profdata"))
        and "::step_mt(int cycles)" in p.read_text()
    ]
    assert len(mains) == 1, mains
    main = mains[0]
    text = main.read_text()
    nthreads = len(re.findall(r"prof_obj \* prof = ref_(\d+)->prof;", text))
    assert nthreads == 24
    text = '#include "phase_trace.h"\n' + text
    text, n = re.subn(
        r"void (\w+)::step_mt\(int cycles\) \{",
        rf'void \1::step_mt(int cycles) {{\n  workcomm::Session phase_trace(cycles,{nthreads},"{mode}");',
        text,
    )
    assert n == 1
    text, n = re.subn(
        r"prof_obj \* prof = ref_(\d+)->prof;",
        lambda m: m[0] + f"\n        auto & phase_thread=phase_trace.threads[{m[1]}];",
        text,
    )
    assert n == nthreads
    if mode == "off":
        sequence = []
        def barrier(_m):
            name, stamp = ("start", "t1a") if len(sequence) % 2 == 0 else ("end", "t4a")
            sequence.append(name)
            return f"auto phase_epoch_{name}=barrier.arrive_only();\n          uint64_t {stamp}=rdtsc();\n          barrier.wait(phase_epoch_{name});"
        text, n = re.subn(r"barrier.arrive_and_wait\(\d+, local_epoch\);", barrier, text)
        assert n == 2 * nthreads
        points = "t1,t1a,t2,t3,t4,t4a,t5,t6"
    else:
        points = "t1,t2,t3,t4,t5,t6"
    text, n = re.subn(
        r"uint64_t t6 = rdtsc\(\);",
        lambda m: m[0] + f"\n          phase_thread.record(i,{{{points}}});",
        text,
    )
    assert n == nthreads
    text, n = re.subn(
        r"thread.join\(\);\n  \}",
        r"thread.join();\n  }\n  phase_trace.write();",
        text,
    )
    assert n == 1
    modified = dest / main.name
    modified.write_text(text)
    shutil.copy2(WORKCOMM / "phase_trace.h", dest / "phase_trace.h")
    commands = subprocess.check_output(["make", "-n", "-B", "sim"], cwd=source, text=True).splitlines()
    oldobj = "objs/" + main.name + ".o"
    cc = next(shlex.split(s) for s in commands if " -c " in s and s.endswith("src/" + main.name))
    cc[cc.index("-o") + 1] = str(dest / "scheduler.o")
    cc[-1] = str(modified)
    cc.extend(["-I" + str(source / "src")])
    link = next(shlex.split(s) for s in commands if " -o sim " in s and " -c " not in s)
    link = [str(dest / "scheduler.o") if s == oldobj else s for s in link]
    link[link.index("-o") + 1] = str(dest / "sim")
    subprocess.run(cc, cwd=source, check=True)
    subprocess.run(link, cwd=source, check=True)
    info = {"mode": mode, "binary": str(dest / "sim"), "binary_sha256": sha256(dest / "sim")}
    (dest / "build.json").write_text(json.dumps(info, indent=2) + "\n")
    return info


def ensure_rocket_traces() -> None:
    for mode in ["off", "on"]:
        build_instrumented(BASE / "sims/rocket_m_1" / mode, WORKCOMM / "builds/rocket_m_1" / mode, mode)
    for ix, (bench, img) in enumerate([("dhry", "div-1-dhrystone.bin"), ("coremark", "div-1-coremark.bin"), ("linux", "linux.bin")]):
        for mode in (["off", "on"] if ix % 2 == 0 else ["on", "off"]):
            stem = f"rocket_m_1.{bench}.{mode}"
            data_path = WORKCOMM / f"{stem}.json"
            log_path = WORKCOMM / f"{stem}.out"
            if data_path.exists() and log_path.exists():
                print("REUSE", stem, flush=True)
                continue
            sim_bin = WORKCOMM / "builds/rocket_m_1" / mode / "sim"
            env = {**os.environ, "MAX_CYCLES": str(CYCLES), "P2SIM_PHASE_TRACE_FILE": str(data_path)}
            cmd = ["taskset", "-c", "28-51", str(sim_bin), str(BASE / "inputs" / img)]
            print("RUN", stem, flush=True)
            with log_path.open("w") as f:
                subprocess.run(cmd, env=env, stdout=f, stderr=subprocess.STDOUT, check=True, timeout=1800)
            print("PASS", stem, flush=True)


def analyze_all() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    summary_rows = []
    for design in ["rocket_m_1", "boom_s_1", "boom_l_1"]:
        for bench in ["linux", "coremark", "dhry"]:
            pair_data = {}
            for mode in ["off", "on"]:
                d = json.load((WORKCOMM / f"{design}.{bench}.{mode}.json").open())
                hz = d["counter_hz"]
                ticks = np.array([t["phase_ticks"] for t in d["threads"]], dtype=float) / d["cycles"]
                us = ticks * 1e6 / hz
                if mode == "off":
                    # [0:arrive_start, 1:wait_start, 2:pull, 3:step, 4:arrive_end, 5:wait_end, 6:push]
                    compute_main = us[:, 3]
                    compute_spec = np.zeros_like(compute_main)
                    compute_total = compute_main
                    state_update = us[:, 2] + us[:, 6] + us[:, 0] + us[:, 4]
                    sync_wait = us[:, 1] + us[:, 5]
                    sync_and_update = state_update + sync_wait
                else:
                    # [0:pull, 1:repair(M), 2:push_arrive, 3:spec(S), 4:wait]
                    compute_main = us[:, 1]
                    compute_spec = us[:, 3]
                    compute_total = compute_main + compute_spec
                    state_update = us[:, 0] + us[:, 2]
                    sync_wait = us[:, 4]
                    sync_and_update = state_update + sync_wait
                total = compute_total + sync_and_update
                pair_data[mode] = {
                    "counter_hz": hz,
                    "avg_compute_main_us": float(compute_main.mean()),
                    "avg_compute_spec_us": float(compute_spec.mean()),
                    "avg_compute_total_us": float(compute_total.mean()),
                    "avg_state_update_us": float(state_update.mean()),
                    "avg_sync_wait_us": float(sync_wait.mean()),
                    "avg_sync_and_update_us": float(sync_and_update.mean()),
                    "avg_total_us": float(total.mean()),
                    "per_thread": [
                        {
                            "tid": int(i),
                            "compute_main_us": round(float(compute_main[i]), 4),
                            "compute_spec_us": round(float(compute_spec[i]), 4),
                            "compute_total_us": round(float(compute_total[i]), 4),
                            "state_update_us": round(float(state_update[i]), 4),
                            "sync_wait_us": round(float(sync_wait[i]), 4),
                            "sync_and_update_us": round(float(sync_and_update[i]), 4),
                            "total_us": round(float(total[i]), 4),
                        }
                        for i in range(len(total))
                    ],
                }
            # Identify top-2 straggler threads by baseline compute time
            off_threads = pair_data["off"]["per_thread"]
            on_threads = pair_data["on"]["per_thread"]
            stragglers = sorted(range(len(off_threads)), key=lambda i: off_threads[i]["compute_total_us"], reverse=True)[:2]
            summary_rows.append({
                "design": design,
                "workload": bench,
                "straggler_threads": stragglers,
                "straggler_comparison": [
                    {
                        "tid": tid,
                        "p2sim": off_threads[tid],
                        "s2sim": on_threads[tid],
                    }
                    for tid in stragglers
                ],
                "off": pair_data["off"],
                "on": pair_data["on"],
            })

    (OUT_DIR / "report.json").write_text(json.dumps({
        "branch": "v2-exp",
        "base_commit": "890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1",
        "cases": summary_rows,
    }, indent=2) + "\n")
    print("WROTE", OUT_DIR / "report.json", flush=True)


if __name__ == "__main__":
    ensure_rocket_traces()
    analyze_all()
