#!/usr/bin/env python3
"""Compile/run an RTL read-before-write oracle and a failing RTL assertion.

Run inside the C++ toolchain environment:
  python3 tests/observer-order.py /absolute/path/to/ggsim
Outputs and logs are retained under --output for diagnosis.
"""
import argparse
import json
import pathlib
import subprocess
import signal


def run(command, log, expected=0):
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log.write_text(result.stdout)
    if result.returncode != expected:
        raise RuntimeError(f"{command} exited {result.returncode}, expected {expected}; see {log}")
    return result.stdout


def fixture(fail):
    observe = 'gAssert(false, "intentional RTL failure");' if fail else 'gAssert(q == expected(), "observer read the next cycle"); record(q);'
    return f'''((name . "ObserverOrder") (max-concat . 2)
      (ext-fns "unsigned expected();" "void record(unsigned);")
      (part (states ((name . "q") (width . 32))) (resets)
        (blocks
          ((always . #f) (next 1) (reads 0) (writes)
            (insts (cpp-code (inst . {json.dumps(observe)}))))
          ((always . #f) (next) (reads 0) (writes 0)
            (insts (write-pre (name . "q") (sid . 0))
              (cpp-code (inst . "q = q + 1;"))
              (write-post (name . "q") (sid . 0) (acts 0 1)))))))'''


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('generator', type=pathlib.Path)
    ap.add_argument('--output', type=pathlib.Path, default=pathlib.Path('build/observer-order'))
    args = ap.parse_args()
    generator = args.generator.resolve()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)
    driver = root / 'driver.cc'
    driver.write_text('''#include "ObserverOrder.h"
#include <cstdlib>
static unsigned seen = 0;
unsigned expected() { return seen; }
void record(unsigned q) {
    if (q != seen) std::abort();
    ++seen;
}
int main(int argc, char**) {
    ObserverOrder sim;
    sim.init();
    // Exercise repeated calls, odd/even batches, and the last-cycle drain.
    if (argc > 1) { sim.step(1); sim.step(3); sim.step(4); }
    else { sim.step_mt(1); sim.step_mt(3); sim.step_mt(4); }
    if (seen != 8) return 7;
}
''')
    for fail in (False, True):
        design = root / ('fail.sexp' if fail else 'order.sexp')
        design.write_text(fixture(fail))
        for mode, flags in [('base', []), ('spec', ['--spec-sim']),
                            ('control', ['--spec-sim', '--spec-sim-no-speculate'])]:
            out = root / f'{design.stem}-{mode}'
            out.mkdir(exist_ok=True)
            run([str(generator), str(design), str(driver), '-k', '1', '-o', str(out), *flags], out / 'generate.log')
            # NDEBUG must not silence RTL assertions.
            run(['make', '-C', str(out), '-j2', 'CPPFLAGS=-DNDEBUG'], out / 'compile.log')
            for method, extra in [('mt', []), ('serial', ['serial'])]:
                output = run([str(out / 'sim'), *extra], out / f'{method}.log', expected=-signal.SIGABRT if fail else 0)
                if fail and 'intentional RTL failure' not in output:
                    raise RuntimeError(f'failure did not come from RTL assertion: {out}')
            print(f'PASS {design.stem}-{mode}', flush=True)


if __name__ == '__main__':
    main()
