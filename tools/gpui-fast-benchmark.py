import os, pathlib, plistlib, shutil, subprocess, sys, time, json
repo=pathlib.Path(__file__).resolve().parents[1]
backend,route=sys.argv[1:3]
base=repo/'target/gpui-fast-benchmark'
base.mkdir(exist_ok=True)
app=base/'Kairo Benchmark.app'; mac=app/'Contents/MacOS'; mac.mkdir(parents=True,exist_ok=True)
shutil.copy2(os.environ.get('KAIRO_BENCH_BINARY',str(repo/'target/release/kairo')),mac/'KairoBenchmark')
(app/'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable':'KairoBenchmark','CFBundleIdentifier':'com.kairo.backend-benchmark','CFBundleName':'Kairo Benchmark','CFBundlePackageType':'APPL','NSHighResolutionCapable':True,'NSPrincipalClass':'NSApplication','LSMinimumSystemVersion':'13.0'}))
report=base/f'{backend}-{route}.json'
if report.exists(): report.unlink()
started=report.with_suffix('.started')
if started.exists(): started.unlink()
# KAIRO_BIBLE_DB and KAIRO_ENGINE_BINARY, when set, override the packaged inputs.
env=dict(os.environ,KAIRO_BENCH_ROOT=str(base/'profile'),KAIRO_BENCH_ROUTE=route,KAIRO_BENCH_REPORT=str(report))
env.setdefault('KAIRO_BIBLE_DB',str(repo/'target/package-macos/inputs/resources/bible.db'))
env.setdefault('KAIRO_ENGINE_BINARY',str(repo/'target/package-macos/out-arm64/Kairo.app/Contents/MacOS/kairo-engine'))
if route=='stream':
    env['KAIRO_BENCH_STREAM']='1'
    env['KAIRO_STT_REPLAY_PATH']=str(base/'replay.jsonl')
log=open(base/f'{backend}-{route}.log','w')
p=subprocess.Popen([str(mac/'KairoBenchmark')],env=env,stdout=log,stderr=log)
(base/'pid').write_text(str(p.pid))
print(json.dumps({'pid':p.pid,'report':str(report)}),flush=True)
# Exact process CPU delta from ps accumulated CPU time, excluding ten-second warmup.
def cpu():
    value=subprocess.check_output(['ps','-p',str(p.pid),'-o','time='],text=True).strip()
    parts=value.replace('-',':').split(':')
    total=0
    for part in parts: total=total*60+float(part)
    return total
deadline=time.monotonic()+120
while not started.exists():
    if p.poll() is not None: raise SystemExit('Benchmark app exited before measuring; inspect '+log.name)
    if time.monotonic()>deadline: p.terminate(); raise SystemExit('Benchmark start timed out')
    time.sleep(.1)
start=cpu(); at=time.monotonic()
time.sleep(30); end=cpu(); elapsed=time.monotonic()-at
(base/f'{backend}-{route}-cpu.json').write_text(json.dumps({'pid':p.pid,'seconds':elapsed,'cpu_seconds':end-start,'cpu_percent_one_core':100*(end-start)/elapsed}))
print((base/f'{backend}-{route}-cpu.json').read_text(),flush=True)
deadline=time.monotonic()+10
while not report.exists() and p.poll() is None and time.monotonic()<deadline: time.sleep(.1)
valid = False
if report.exists():
    result=json.loads(report.read_text())
    print(json.dumps(result),flush=True)
    valid = route not in ('lyrics','media','stream') or bool(result.get('draw'))
    if not valid: print('INVALID: workload produced no drawn frames',flush=True)
else: print('Frame report missing',flush=True)
p.terminate()
try: p.wait(timeout=5)
except subprocess.TimeoutExpired: p.kill(); p.wait()

if not valid: raise SystemExit(1)
