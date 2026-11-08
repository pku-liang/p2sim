#!/usr/bin/env python3
"""
Experiment 4: Ablation Study on Large BOOM (boom_l_1, 24 Threads)
Repository: git@github.com:pku-liang/p2sim.git
Branch: v2-exp
Base Commit: 890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1
Design: Large BOOM (boom_l_1), k=24 threads pinned to physical cores 28-51
Workloads: BootLinux (linux.bin), CoreMark (div-1-coremark.bin), Dhrystone (div-1-dhrystone.bin)
Variants:
  1. p2sim      : Baseline P2SIM (single buffer, 2 blocking barriers/cycle, no speculation)
  2. dual_buf   : +Split-Barrier Dual-Buffer (--spec-sim --spec-sim-no-speculate)
  3. spec_naive : +Cross-Cycle S/M Speculation without decoupled activation (sets __s_act_mask on all pending writes)
  4. full_s2sim : Full S2SIM (--spec-sim with decoupled pending state coherence & fresh activation)
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import time

ROOT = Path.home() / "pro/p2sim-next/p2sim"
BASE = ROOT / "build/spec-matrix-20261104"
BUILD_DIR = ROOT / "build/ablation-boom-l-20261108"
OUT_DIR = ROOT / "results/eval-ablation-boom-l-20261108"
CYCLES = 5_000_000
REPEATS = 3
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
    clean = audit.SAMPLE.sub("", raw).replace("\r", "")
    clean = re.sub(
        r"load program to memory\n  program_size=\d+\nset reset to 1\nset reset to 0\nstart simulation: max_cycles=\d+\n",
        "",
        clean,
    )
    rows = clean.splitlines()
    if not clean.endswith("\n"):
        rows = rows[:-1]
    return [re.sub(r"^\[\s*[0-9.]+\]\s*", "", s) for s in rows if s.strip()]


def prepare_binaries() -> dict[str, Path]:
    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    bins = {
        "p2sim": BASE / "sims/boom_l_1/off/sim",
        "full_s2sim": BASE / "sims/boom_l_1/on/sim",
    }
    assert bins["p2sim"].exists() and bins["full_s2sim"].exists()

    # 1. Build dual_buf (--spec-sim --spec-sim-no-speculate)
    dual_dir = BUILD_DIR / "dual_buf"
    if not (dual_dir / "sim").exists():
        shutil.rmtree(dual_dir, ignore_errors=True)
        dual_dir.mkdir(parents=True, exist_ok=True)
        gen_cmd = [
            str(ROOT / "target/release/ggsim"),
            str(BASE / "inputs/boom_l_1.ggsim.sexp"),
            str(ROOT / "emu/emu.cc"),
            "-lfesvr",
            "-o",
            str(dual_dir),
            "-k",
            "24",
            "--prof-rdtsc-avg",
            "--spec-sim",
            "--spec-sim-no-speculate",
        ]
        print("GENERATE dual_buf", flush=True)
        with (BUILD_DIR / "dual_buf.generate.log").open("w") as f:
            subprocess.run(gen_cmd, stdout=f, stderr=subprocess.STDOUT, check=True)
        print("COMPILE dual_buf", flush=True)
        with (BUILD_DIR / "dual_buf.make.log").open("w") as f:
            subprocess.run(
                ["make", "-j24", "-C", str(dual_dir)],
                cwd=ROOT,
                stdout=f,
                stderr=subprocess.STDOUT,
                check=True,
            )
    bins["dual_buf"] = dual_dir / "sim"

    # 2. Build spec_naive (copy full_s2sim sources & non-thread objects, remove `if(push_changed_...)` guard in thread_*.cc)
    naive_dir = BUILD_DIR / "spec_naive"
    if not (naive_dir / "sim").exists():
        shutil.rmtree(naive_dir, ignore_errors=True)
        (naive_dir / "src").mkdir(parents=True, exist_ok=True)
        (naive_dir / "objs").mkdir(parents=True, exist_ok=True)
        src_on = BASE / "sims/boom_l_1/on"
        shutil.copy2(src_on / "Makefile", naive_dir / "Makefile")
        for sfile in (src_on / "src").iterdir():
            if sfile.is_file():
                text = sfile.read_text()
                if sfile.name.startswith("thread_") and sfile.name.endswith(".cc"):
                    # Replace guarded activation `if(push_changed_X) { io_shared->__s_act_mask[...] |= ...; }`
                    # with unconditional activation on every pending dual-buffer sync write
                    text_new, count = re.subn(
                        r"if\(push_changed_\d+\) \{\s*(io_shared->__s_act_mask\[\d+\] \|= \(1 << \d+\);)\s*\}",
                        r"\1",
                        text,
                    )
                    assert count > 0, f"No push_changed guard found in {sfile}"
                    (naive_dir / "src" / sfile.name).write_text(text_new)
                else:
                    shutil.copy2(sfile, naive_dir / "src" / sfile.name)
                    obj_file = src_on / "objs" / (sfile.name + ".o")
                    if obj_file.exists():
                        shutil.copy2(obj_file, naive_dir / "objs" / obj_file.name)
                        # Ensure object timestamp is newer than copied source
                        now = time.time()
                        os.utime(naive_dir / "objs" / obj_file.name, (now + 10, now + 10))
        print("COMPILE spec_naive", flush=True)
        with (BUILD_DIR / "spec_naive.make.log").open("w") as f:
            subprocess.run(
                ["make", "-j24", "-C", str(naive_dir)],
                cwd=ROOT,
                stdout=f,
                stderr=subprocess.STDOUT,
                check=True,
            )
    bins["spec_naive"] = naive_dir / "sim"
    return bins


def run_ablation(bins: dict[str, Path]) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    report_path = OUT_DIR / "report.json"
    if report_path.exists():
        rep = json.loads(report_path.read_text())
        trials = rep.get("trials", [])
    else:
        trials = []

    variants = ["p2sim", "dual_buf", "spec_naive", "full_s2sim"]
    linux_ref = json.loads((BASE / "boom_l_1.linux-reference.json").read_text())

    for bench, img_name in WORKLOADS:
        image = BASE / "inputs" / img_name
        for r in range(1, REPEATS + 1):
            # Rotate order across repeats to cancel any host drift
            order = variants[(r - 1) % 4 :] + variants[: (r - 1) % 4]
            for v in order:
                if any(t["workload"] == bench and t["repeat"] == r and t["variant"] == v and t.get("valid") for t in trials):
                    print(f"REUSE {bench} r{r} {v}", flush=True)
                    continue
                log = OUT_DIR / f"boom_l_1.{bench}.r{r}.{v}.out"
                env = {**os.environ, "MAX_CYCLES": str(CYCLES)}
                env.pop("PROF_FILE", None)
                cmd = ["taskset", "-c", "28-51", str(bins[v]), str(image)]
                t0 = time.time()
                load_before = os.getloadavg()
                print(f"RUN {bench} r{r} {v}", flush=True)
                with log.open("w") as f:
                    res = subprocess.run(cmd, env=env, stdout=f, stderr=subprocess.STDOUT, timeout=1800)
                wall = time.time() - t0
                raw = log.read_text(errors="replace")
                if res.returncode != 0:
                    raise RuntimeError(f"{bench} r{r} {v} exited {res.returncode}")
                samples = audit.SAMPLE.findall(raw)
                if not samples or int(samples[-1][0]) != CYCLES:
                    raise RuntimeError(f"{bench} r{r} {v} cycle mismatch")
                if bench == "linux":
                    assert linux_lines(raw) == linux_ref, f"Linux output mismatch for {v}"
                    assert not re.search(r"Kernel panic|Oops:|BUG:|Assertion.*failed", raw)
                    khz = float(samples[-1][1])
                else:
                    khz = audit.validate(raw, bench, CYCLES)
                trials.append({
                    "design": "boom_l_1",
                    "threads": 24,
                    "workload": bench,
                    "repeat": r,
                    "variant": v,
                    "binary": str(bins[v]),
                    "binary_sha256": sha256(bins[v]),
                    "log": log.name,
                    "log_sha256": sha256(log),
                    "started": t0,
                    "seconds": wall,
                    "load_before": load_before,
                    "load_after": os.getloadavg(),
                    "khz": khz,
                    "valid": True,
                })
                print(f"PASS {bench} r{r} {v}: {khz:.3f} kHz ({wall:.1f}s)", flush=True)
                report_path.write_text(json.dumps({"trials": trials}, indent=2) + "\n")

    summary = []
    for bench, _ in WORKLOADS:
        meds = {}
        for v in variants:
            vals = [t["khz"] for t in trials if t["workload"] == bench and t["variant"] == v and t["valid"]]
            meds[v] = round(statistics.median(vals), 3)
        summary.append({
            "design": "boom_l_1",
            "threads": 24,
            "workload": bench,
            "p2sim_khz": meds["p2sim"],
            "dual_buf_khz": meds["dual_buf"],
            "spec_naive_khz": meds["spec_naive"],
            "full_s2sim_khz": meds["full_s2sim"],
            "dual_buf_over_p2sim": round(meds["dual_buf"] / meds["p2sim"], 3),
            "spec_naive_over_p2sim": round(meds["spec_naive"] / meds["p2sim"], 3),
            "full_s2sim_over_p2sim": round(meds["full_s2sim"] / meds["p2sim"], 3),
            "decoupled_gain_over_naive": round(meds["full_s2sim"] / meds["spec_naive"], 3),
        })

    report = {
        "branch": "v2-exp",
        "base_commit": "890d7205ee5a6bb5d48cfe5de9c495d6b9be4fa1",
        "design": "boom_l_1",
        "threads": 24,
        "cycles": CYCLES,
        "repeats": REPEATS,
        "summary": summary,
        "trials": trials,
    }
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print("SUMMARY:", json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    bins = prepare_binaries()
    run_ablation(bins)
