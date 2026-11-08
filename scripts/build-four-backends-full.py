#!/usr/bin/env python3
"""
Build complete 4-backend (origin P2SIM, spec S2SIM, GSIM, Verilator) binaries
for Rocket (rocket_m_1), Small BOOM (boom_s_1), and Large BOOM (boom_l_1)
across thread counts [1, 2, 4, 8, 16, 24, 28, 32] in build/scaling-four-backends-20261029.
"""
from concurrent.futures import ThreadPoolExecutor
from threading import Lock
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

ROOT = Path.home() / "pro/p2sim-next/p2sim"
OUT = ROOT / "build/scaling-four-backends-20261029"
EXT_VLT = ROOT / "build/scaling-28-32-20261108/sims"
THREADS = [1, 2, 4, 8, 16, 24, 28, 32]
DESIGNS = ["rocket_m_1", "boom_s_1", "boom_l_1"]


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main() -> None:
    os.chdir(ROOT)
    manifest_path = OUT / "builds.json"
    records = json.loads(manifest_path.read_text()) if manifest_path.exists() else []
    lock = Lock()

    def save():
        manifest_path.write_text(json.dumps(records, indent=2) + "\n")

    # 1. Reuse already-built Verilator T28/T32 binaries from build/scaling-28-32-20261108/sims
    for d in DESIGNS:
        for n in [28, 32]:
            key = f"{d}/verilator/T{n}"
            dest = OUT / "sims" / key
            dest.mkdir(parents=True, exist_ok=True)
            bin_dst = dest / "VTestHarness"
            bin_src = EXT_VLT / d / f"T{n}/verilator/VTestHarness"
            if not bin_dst.exists() and bin_src.exists():
                shutil.copy2(bin_src, bin_dst)
            if bin_dst.exists() and not any(r.get("key") == key and r.get("status") == "ok" for r in records):
                records.append({
                    "key": key,
                    "design": d,
                    "backend": "verilator",
                    "threads": n,
                    "status": "ok",
                    "binary": str(bin_dst),
                    "binary_sha256": sha256(bin_dst),
                    "input_sha256": sha256(OUT / "inputs" / f"{d}.sv"),
                    "generator_sha256": sha256(ROOT / ".pixi/envs/default/bin/verilator_bin"),
                })
                save()
                print("IMPORTED", key, flush=True)

    # 2. Build missing origin (original P2SIM) and spec (S2SIM) binaries
    tasks = []
    for d in DESIGNS:
        for mode in ["origin", "spec"]:
            for n in THREADS:
                key = f"{d}/{mode}/T{n}"
                dest = OUT / "sims" / key
                bin_path = dest / "sim"
                old = next((r for r in records if r.get("key") == key and r.get("status") == "ok"), None)
                if old and bin_path.exists() and sha256(bin_path) == old.get("binary_sha256"):
                    continue
                tasks.append((d, mode, n, key, dest, bin_path))

    print(f"Building {len(tasks)} origin/spec targets...", flush=True)

    def build_one(task):
        d, mode, n, key, dest, bin_path = task
        dest.mkdir(parents=True, exist_ok=True)
        generator = OUT / "tools" / f"{mode}-generator"
        assert generator.exists(), f"Missing {generator}"
        gen = [
            str(generator),
            str(OUT / "inputs" / f"{d}.ggsim.sexp"),
            str(OUT / "spec-source/emu/emu.cc"),
            "-lfesvr",
            "-o",
            str(dest),
            "-k",
            str(n),
        ]
        if mode == "spec":
            gen.append("--spec-sim")
        make = ["make", "-j12", "-C", str(dest)]
        stem = OUT / "logs" / f"{d}.{mode}.T{n}"
        t0 = time.time()
        print("BUILD", key, flush=True)
        with Path(str(stem) + ".generate.log").open("w") as f:
            r1 = subprocess.run(gen, stdout=f, stderr=subprocess.STDOUT, timeout=1800, env={**os.environ, "RAYON_NUM_THREADS": "6"})
        if r1.returncode != 0:
            raise RuntimeError(f"gen failed for {key}")
        with Path(str(stem) + ".compile.log").open("w") as f:
            r2 = subprocess.run(make, stdout=f, stderr=subprocess.STDOUT, timeout=1800)
        if r2.returncode != 0:
            raise RuntimeError(f"make failed for {key}")
        assert bin_path.exists(), f"Missing binary {bin_path}"
        elapsed = time.time() - t0
        rec = {
            "key": key,
            "design": d,
            "backend": mode,
            "threads": n,
            "status": "ok",
            "binary": str(bin_path),
            "binary_sha256": sha256(bin_path),
            "input_sha256": sha256(OUT / "inputs" / f"{d}.ggsim.sexp"),
            "generator_sha256": sha256(generator),
            "commands": [gen, make],
            "seconds": elapsed,
        }
        with lock:
            records[:] = [r for r in records if r.get("key") != key]
            records.append(rec)
            save()
        print(f"OK {key} ({elapsed:.1f}s)", flush=True)

    with ThreadPoolExecutor(max_workers=4) as pool:
        futs = [pool.submit(build_one, t) for t in tasks]
        for fut in futs:
            fut.result()

    print("ALL BUILDS COMPLETE", flush=True)


if __name__ == "__main__":
    main()
