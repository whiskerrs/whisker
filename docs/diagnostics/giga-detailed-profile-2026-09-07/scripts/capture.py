import subprocess,time,sys,json,re
from pathlib import Path
mode,action,label=sys.argv[1:];root=Path('/tmp/giga-detailed-profile');out=root/label;out.mkdir(exist_ok=True)
adb=['adb','-s','emulator-5554'];pkg='jp.co.chanto.giga'
def shell(*a):return subprocess.check_output(adb+['shell',*a],text=True).strip()
def shot(name):
 with (out/name).open('wb') as f:subprocess.run(adb+['exec-out','screencap','-p'],stdout=f,check=True)
pid=int(shell('pidof',pkg));shot('before.png');shell('dumpsys','gfxinfo',pkg,'reset');duration=10 if action=='scroll' else 7
remote=f'/data/misc/perfetto-traces/codex-detail-{label}.pftrace' if mode=='perfetto' else f'files/codex-detail-{label}.data'
start=time.monotonic()
if mode=='none':
 proc=None
elif mode=='perfetto':
 config=Path('/tmp/giga-android-frame-study/config.pbtxt').read_text().replace('duration_ms: 55000',f'duration_ms: {duration*1000}')
 (out/'config.pbtxt').write_text(config)
 r=subprocess.run(adb+['shell','perfetto','--txt','-c','-','-o',remote,'--background-wait'],input=config,text=True,capture_output=True,check=True)
 (out/'recorder.log').write_text(r.stdout+r.stderr)
 proc=None
else:
 log=(out/'recorder.log').open('w')
 proc=subprocess.Popen(adb+['shell','run-as',pkg,'simpleperf','record','-p',str(pid),'-e','cpu-clock','-f','400','--duration',str(duration),'-g','-o',remote],stdout=log,stderr=log)
time.sleep(.5)
action_start=int(float(shell('cat','/proc/uptime').split()[0])*1e9)
if action=='group-open':shell('input','tap','520','650')
elif action=='reader-open':shell('input','tap','850','1260')
elif action in ['reader-back','group-back']:shell('input','keyevent','4')
elif action=='scroll':
 for _ in range(6):shell('input','swipe','540','2000','540','600','500')
else:raise ValueError(action)
time.sleep(max(0,duration+1-(time.monotonic()-start)))
if proc:
 proc.wait(timeout=5);log.close()
 if proc.returncode:raise RuntimeError('simpleperf failed')
 with (out/'cpu.data').open('wb') as f:subprocess.run(adb+['exec-out','run-as',pkg,'cat',remote],stdout=f,check=True)
 shell('run-as',pkg,'rm',remote)
elif mode=='perfetto':
 subprocess.run(adb+['pull',remote,str(out/'timeline.pftrace')],check=True,capture_output=True);shell('rm',remote)
newpid=shell('pidof',pkg)
shot('after.png');raw=shell('dumpsys','gfxinfo',pkg,'framestats');(out/'gfxinfo.txt').write_text(raw)
def val(p):
 m=re.search(p,raw);return float(m[1]) if m else None
meta={'mode':mode,'action':action,'pid':pid,'pid_after':newpid,'action_start_ns':action_start,'duration_s':duration,'frames':val(r'Total frames rendered: (\d+)'),'jank_percent':val(r'Janky frames: \d+ \(([\d.]+)%\)'),'p95_ms':val(r'95th percentile: (\d+)ms')}
(out/'metadata.json').write_text(json.dumps(meta,indent=2));print(json.dumps(meta),flush=True)
if newpid!=str(pid) or not meta['frames']:raise RuntimeError('Invalid capture: process changed or no frames')
