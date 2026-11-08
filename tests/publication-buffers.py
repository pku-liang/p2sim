#!/usr/bin/env python3
"""Check independent publication buffers across serial/MT and odd/even batches."""
import argparse
import pathlib
import subprocess


def run(command, log):
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log.write_text(result.stdout)
    if result.returncode:
        raise RuntimeError(f'{command} exited {result.returncode}; see {log}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('generator', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('build/publication-buffers'))
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    design = root / 'buffers.sexp'
    design.write_text('''((name . "Buffers") (max-concat . 2) (ext-fns)
      (part (states
        ((name . "input_a") (width . 32) (is-ext . #t))
        ((name . "input_b") (width . 32) (is-ext . #t))
        ((name . "output_a") (width . 32) (is-ext . #t))
        ((name . "output_b") (width . 32) (is-ext . #t))
        ((name . "next_a") (width . 32))
        ((name . "next_b") (width . 32)))
        (resets) (blocks
          ((always . #f) (next 2 3) (reads 0 3) (writes 4)
            (insts (write-pre (name . "next_a") (sid . 4))
              (cpp-code (inst . "next_a = input_a + (output_b & 0u);"))
              (write-post (name . "next_a") (sid . 4) (acts 2))))
          ((always . #f) (next 2 3) (reads 1 2) (writes 5)
            (insts (write-pre (name . "next_b") (sid . 5))
              (cpp-code (inst . "next_b = input_b + (output_a & 0u);"))
              (write-post (name . "next_b") (sid . 5) (acts 3))))
          ((always . #f) (next) (reads 4) (writes 2)
            (insts (write-pre (name . "output_a") (sid . 2))
              (cpp-code (inst . "output_a = next_a;"))
              (write-post (name . "output_a") (sid . 2) (acts 1))))
          ((always . #f) (next) (reads 5) (writes 3)
            (insts (write-pre (name . "output_b") (sid . 3))
              (cpp-code (inst . "output_b = next_b;"))
              (write-post (name . "output_b") (sid . 3) (acts 0)))))))''')
    driver = root / 'driver.cc'
    driver.write_text('''#include "Buffers.h"
#include <cstdlib>
int main() {
    Buffers sim;
    sim.init();
    unsigned a = 0, b = 0;
    unsigned rng = 42;
    for (unsigned i = 0; i < 192; ++i) {
        rng = rng * 1664525u + 1013904223u;
        // Long stable intervals, zero transitions, and simultaneous writes.
        if (i % 11 == 0) { a = (i % 22 == 0) ? 0 : rng; sim.set_input_a(a); }
        if (i % 7 == 0) { b = rng; sim.set_input_b(b); }
        unsigned count = 1 + (i % 4);
        if (i % 3 == 0) sim.step(count); else sim.step_mt(count);
        if (sim.get_output_a() != a || sim.get_output_b() != b) {
            fprintf(stderr, "batch %u: got %u/%u expected %u/%u\\n", i,
                sim.get_output_a(), sim.get_output_b(), a, b);
            return 1;
        }
    }
}
''')
    for threads in (2,):
        for mode, flags in [('base', []), ('spec', ['--spec-sim']),
                            ('control', ['--spec-sim', '--spec-sim-no-speculate'])]:
            out = root / f'T{threads}-{mode}'
            out.mkdir(exist_ok=True)
            run([str(args.generator.resolve()), str(design), str(driver), '-k', str(threads), '-o', str(out), *flags], out / 'generate.log')
            run(['make', '-C', str(out), '-j2'], out / 'compile.log')
            run([str(out / 'sim')], out / 'run.log')
            print(f'PASS T{threads}-{mode}', flush=True)


if __name__ == '__main__':
    main()
