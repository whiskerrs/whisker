import os,signal,subprocess,time,shutil
from pathlib import Path
out=Path('/tmp/giga-detailed-profile');giga=Path('/Users/itome/Projects/github.com/chantoinc/giga')
originals={giga/n:(giga/n).read_bytes() for n in ['Cargo.toml','Cargo.lock']}
for p,b in originals.items():(out/(p.name+'.original')).write_bytes(b)
proc=None
try:
 patch='\n[patch.crates-io]\n'+''.join(f'{c} = {{ path = "{out/c}" }}\n' for c in ['whisker-runtime','whisker-engine','whisker-driver'])+'whisker-plugin = { path = "/private/tmp/whisker-ios-run-macro-validation/crates/whisker-plugin" }\n'
 (giga/'Cargo.toml').write_bytes(originals[giga/'Cargo.toml']+patch.encode())
 with (out/'build.log').open('w') as f:
  proc=subprocess.Popen(['cargo','whisker','run','android','--no-tui'],cwd=giga,stdout=f,stderr=subprocess.STDOUT,start_new_session=True,env={**os.environ,'ANDROID_SERIAL':'emulator-5554'})
  deadline=time.monotonic()+600
  while time.monotonic()<deadline:
   time.sleep(1);s=(out/'build.log').read_text()
   if 'initial done' in s:print('Built and installed',flush=True);break
   if proc.poll() is not None or 'initial build failed' in s:raise RuntimeError('Build failed, see build.log')
  else:raise TimeoutError('Build timeout')
 shutil.copyfile(giga/'gen/android/app/build/outputs/apk/debug/app-debug.apk',out/'profile.apk')
 shutil.copyfile(giga/'target/aarch64-linux-android/debug/libgiga.so',out/'profile-unstripped.so')
finally:
 if proc and proc.poll() is None:
  os.killpg(proc.pid,signal.SIGTERM)
  try:proc.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
 for p,b in originals.items():p.write_bytes(b)
