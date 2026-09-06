from pathlib import Path
import zipfile,subprocess,hashlib,json
r=Path('/tmp/giga-detailed-profile');z=zipfile.ZipFile(r/'profile.apk');n=[n for n in z.namelist() if n.endswith('/libgiga.so')][0];(r/'apk.so').write_bytes(z.read(n))
objcopy='/Users/itome/Library/Android/sdk/ndk/29.0.14206865/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-objcopy';h={}
for name in ['apk','profile-unstripped']:
 subprocess.run([objcopy,'--dump-section',f'.text={r/name}.text',str(r/(name+'.so')),str(r/'discard.so')],check=True)
 h[name]=hashlib.sha256((r/(name+'.text')).read_bytes()).hexdigest()
(r/'binary-identity.json').write_text(json.dumps(h,indent=2));print(h);assert len(set(h.values()))==1
