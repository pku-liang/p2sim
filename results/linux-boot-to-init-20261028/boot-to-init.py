import hashlib,json,os,re,signal,subprocess,time
from pathlib import Path
root=Path.home()/'pro/p2sim-next/p2sim'
os.chdir(root)
out=root/'build/linux-boot-to-init-20261028'
out.mkdir(exist_ok=False)
pattern=re.compile(r'cycle=\d+, last_freq=[0-9.]+ kHz, freq=[0-9.]+ kHz\n?')
variants={'baseline':root/'build/rocket_m_1.T24.base/sim','fixed':root/'build/correctness-20261028/rocket.T24.sparse/sim'}
report={'image_sha256':hashlib.sha256((root/'exe/linux.bin').read_bytes()).hexdigest(),'runs':{}}
for name,sim in variants.items():
    logfile=out/(name+'.out'); start=time.time();reason='cycle_limit_without_userspace'
    with logfile.open('w') as stream:
        p=subprocess.Popen(['pixi','run','env','MAX_CYCLES=100000000','stdbuf','-oL','-eL',str(sim),'exe/linux.bin'],stdout=stream,stderr=subprocess.STDOUT,start_new_session=True)
        while p.poll() is None:
            raw=logfile.read_text(errors='replace');clean=pattern.sub('',raw)
            if 'Hello, RISC-V World!\nhanging\n' in clean:
                reason='stopped_after_expected_userspace_output';os.killpg(p.pid,signal.SIGTERM);break
            if time.time()-start>1800:
                reason='wall_timeout';os.killpg(p.pid,signal.SIGTERM);break
            time.sleep(1)
        try: status=p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid,signal.SIGKILL);status=p.wait()
    raw=logfile.read_text(errors='replace');clean=pattern.sub('',raw)
    (out/(name+'.normalized.txt')).write_text(clean)
    bad=re.findall(r'^.*(?:Kernel panic|Oops:|BUG:|Assertion.*failed).*$' ,clean,re.M)
    samples=re.findall(r'cycle=(\d+), last_freq=[0-9.]+ kHz, freq=([0-9.]+) kHz',raw)
    success=reason=='stopped_after_expected_userspace_output' and not bad
    report['runs'][name]={'success':success,'reason':reason,'returncode':status,'elapsed_seconds':time.time()-start,'last_sample':samples[-1] if samples else None,'errors':bad,'binary_sha256':hashlib.sha256(sim.read_bytes()).hexdigest(),'log_sha256':hashlib.sha256(logfile.read_bytes()).hexdigest()}
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(name,json.dumps(report['runs'][name]),flush=True)
report['normalized_output_identical']=(out/'baseline.normalized.txt').read_bytes()==(out/'fixed.normalized.txt').read_bytes()
report['passed']=all(r['success'] for r in report['runs'].values()) and report['normalized_output_identical']
(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print('PASS' if report['passed'] else 'FAIL',flush=True)
