#!/usr/bin/env python3
import argparse
import struct
from pathlib import Path


BASELINE_LABELS = ("count", "barrier_start", "pull", "step", "barrier_end", "push")
SPEC_LABELS = ("count", "pull", "repair", "push_arrive", "spec", "wait")


def read_exact(data: memoryview, offset: int, size: int) -> tuple[memoryview, int]:
    end = offset + size
    if end > len(data):
        raise ValueError("truncated profile")
    return data[offset:end], end


def unpack_from(fmt: str, data: memoryview, offset: int):
    size = struct.calcsize(fmt)
    raw, offset = read_exact(data, offset, size)
    return struct.unpack(fmt, raw), offset


def parse_profile(path: Path):
    data = memoryview(path.read_bytes())
    (mh_bits, mh_width), offset = unpack_from("<II", data, 0)
    (thread_count,), offset = unpack_from("<Q", data, offset)
    threads = []
    hash_bytes = mh_bits // 8 * mh_width

    for _ in range(thread_count):
        rdtsc, offset = unpack_from("<6Q", data, offset)
        (act_count,), offset = unpack_from("<Q", data, offset)
        _, offset = read_exact(data, offset, act_count * 4)
        (hash_count,), offset = unpack_from("<Q", data, offset)
        _, offset = read_exact(data, offset, hash_count * hash_bytes)
        (tick_count,), offset = unpack_from("<Q", data, offset)
        _, offset = read_exact(data, offset, tick_count * 8)
        threads.append(rdtsc)

    return threads


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("profile", type=Path)
    parser.add_argument("--mode", choices=("baseline", "spec"), default="baseline")
    args = parser.parse_args()

    labels = BASELINE_LABELS if args.mode == "baseline" else SPEC_LABELS
    threads = parse_profile(args.profile)
    print("thread\t" + "\t".join(labels[1:]) + "\ttotal")
    for thread_id, values in enumerate(threads):
        count = values[0]
        avgs = [value / count if count else 0.0 for value in values[1:]]
        print(
            f"{thread_id}\t"
            + "\t".join(f"{value:.1f}" for value in avgs)
            + f"\t{sum(avgs):.1f}"
        )


if __name__ == "__main__":
    main()
