#!/usr/bin/env python3
"""Run adjacent baseline/previous/candidate trials with functional and speed gates.

Invoke inside the simulator's pixi environment. Every run is independent and
records its exit status, output, binary hash, and measured frequency.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time

SAMPLE = re.compile(r'cycle=(\d+), last_freq=[0-9.eE+-]+ kHz, freq=([0-9.eE+-]+) kHz\n?')


def validate(text, benchmark, cycles):
    samples = SAMPLE.findall(text)
    if not samples or int(samples[-1][0]) != cycles:
        raise ValueError('simulation did not finish the requested cycle count')
    clean = SAMPLE.sub('', text)
    if benchmark == 'dhry':
        if 'Dhrystones per Second:' not in clean:
            raise ValueError('Dhrystone did not finish')
        section = clean.split('inal values of the variables used in the benchmark:')[-1]
        pairs = re.findall(r'^\s*([^:\n]+):\s*([^\n]+)\n\s*should be:\s*([^\n]+)', section, re.M)
        if len(pairs) != 22:
            raise ValueError(f'expected 22 Dhrystone final values, got {len(pairs)}')
        pointers = []
        for name, actual, expected in pairs:
            actual, expected = actual.strip(), expected.strip()
            if expected.startswith('(implementation-dependent)'):
                pointers.append(actual)
                continue
            if expected == 'Number_Of_Runs + 10':
                runs = re.search(r'Trying (\d+) runs', clean)
                if not runs:
                    raise ValueError('missing Dhrystone run count')
                expected = str(int(runs[1]) + 10)
            if actual != expected:
                raise ValueError(f'{name.strip()}: {actual!r} != {expected!r}')
        if len(pointers) != 2 or pointers[0] != pointers[1]:
            raise ValueError('Dhrystone pointer relationship failed')
    else:
        for name, value in [('crclist', 'e714'), ('crcmatrix', '1fd7'), ('crcstate', '8e3a')]:
            if not re.search(rf'\[0\]{name}\s*:\s*0x{value}\b', clean):
                raise ValueError(f'CoreMark {name} mismatch')
        errors = [line for line in clean.splitlines() if 'ERROR!' in line]
        if any('Must execute for at least 10 secs for a valid result!' not in line for line in errors):
            raise ValueError(f'CoreMark errors: {errors}')
        if 'Errors detected' in clean and not errors:
            raise ValueError('unexplained CoreMark error')
    return float(samples[-1][1])


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline', 'previous', 'candidate'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--images', type=Path, default=Path('exe'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cycles', type=int, default=5000000)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--minimum-speedup', type=float, default=1.30)
    parser.add_argument('--minimum-retention', type=float, default=1.0)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {name: getattr(args, name).resolve() for name in ('baseline', 'previous', 'candidate')}
    report = {'cycles': args.cycles, 'repeats': args.repeats,
              'affinity': sorted(os.sched_getaffinity(0)),
              'binaries': {name: {'path': str(path), 'sha256': sha256(path)} for name, path in binaries.items()},
              'trials': [], 'summary': {}}
    def save():
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    save()
    for benchmark, image in [('dhry', 'div-1-dhrystone.bin'), ('coremark', 'div-1-coremark.bin')]:
        for repeat in range(args.repeats):
            # Rotate order to distribute slow host-load drift across variants.
            order = list(binaries)
            order = order[repeat % 3:] + order[:repeat % 3]
            for name in order:
                log = args.output / f'{benchmark}-{repeat + 1}-{name}.out'
                command = [str(binaries[name]), str((args.images / image).resolve())]
                env = {**os.environ, 'MAX_CYCLES': str(args.cycles)}
                env.pop('PROF_FILE', None)
                start = time.time()
                with log.open('w') as stream:
                    result = subprocess.run(command, env=env, stdout=stream, stderr=subprocess.STDOUT, timeout=600)
                trial = {'benchmark': benchmark, 'repeat': repeat + 1, 'variant': name,
                         'started': start, 'seconds': time.time() - start,
                         'returncode': result.returncode, 'log': log.name,
                         'log_sha256': sha256(log), 'load': os.getloadavg()}
                report['trials'].append(trial)
                save()
                if result.returncode:
                    raise RuntimeError(f'simulator failed: {log}')
                trial['frequency_khz'] = validate(log.read_text(), benchmark, args.cycles)
                save()
                print(f'{benchmark} #{repeat + 1} {name}: {trial["frequency_khz"]:.3f} kHz; correctness PASS', flush=True)
        medians = {name: statistics.median(t['frequency_khz'] for t in report['trials']
                   if t['benchmark'] == benchmark and t['variant'] == name) for name in binaries}
        report['summary'][benchmark] = {'median_khz': medians,
                'speedup': medians['candidate'] / medians['baseline'],
                'previous_speedup': medians['previous'] / medians['baseline'],
                'retention': medians['candidate'] / medians['previous']}
        save()
    report['passed'] = all(s['speedup'] >= args.minimum_speedup and s['retention'] >= args.minimum_retention
                           for s in report['summary'].values())
    save()
    print(json.dumps(report['summary'], indent=2), flush=True)
    if not report['passed']:
        raise SystemExit('Performance gate failed; inspect report.json')


if __name__ == '__main__':
    main()
