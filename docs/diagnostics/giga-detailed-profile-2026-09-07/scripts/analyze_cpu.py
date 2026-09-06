import sys,subprocess,json,collections
from pathlib import Path
ndk=Path('/Users/itome/Library/Android/sdk/ndk/29.0.14206865');sys.path.insert(0,str(ndk/'simpleperf'))
from simpleperf_report_lib import GetReportLib
root=Path('/tmp/giga-detailed-profile');all_samples={};addrs=set()
for d in sorted(root.glob('simpleperf-*')):
 if not (d/'cpu.data').exists():continue
 lib=GetReportLib(str(d/'cpu.data'));samples=[]
 while True:
  s=lib.GetNextSample()
  if s is None:break
  sym=lib.GetSymbolOfCurrentSample();chain=lib.GetCallChainOfCurrentSample();syms=[sym]+[chain.entries[i].symbol for i in range(chain.nr)];frames=[]
  for sym in syms:
   if 'libgiga.so' in sym.dso_name:addr=hex(sym.vaddr_in_file);addrs.add(addr);frames.append(addr)
   else:frames.append(sym.symbol_name+' ['+sym.dso_name.split('/')[-1]+']')
  samples.append({'tid':s.tid,'comm':s.thread_comm,'period':s.period,'time':s.time,'frames':frames})
 lib.Close();all_samples[d.name]=samples
addrs=sorted(addrs)
r=subprocess.check_output([str(ndk/'toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-addr2line'),'-f','-C','-e',str(root/'profile-unstripped.so')],input='\n'.join(addrs)+'\n',text=True).splitlines()
resolved={a:r[i*2] for i,a in enumerate(addrs)}
(root/'symbols.json').write_text(json.dumps({a:r[i*2:i*2+2] for i,a in enumerate(addrs)},indent=2))
for name,samples in all_samples.items():
 d=root/name;pid=json.loads((d/'metadata.json').read_text())['pid'];inc=collections.Counter();own=collections.Counter();threads=collections.Counter();main=[]
 for s in samples:
  s['frames']=[resolved.get(x,x) for x in s['frames']];threads[s['tid']]+=s['period']
  if s['tid']==pid:
   main.append(s);own[s['frames'][0]]+=s['period']
   for f in set(s['frames']):inc[f]+=s['period']
 total=sum(s['period'] for s in main)
 summary={'main_tid':pid,'main_samples':len(main),'all_samples':len(samples),'sampled_main_cpu_ms':total/1e6,'threads_cpu_ms':{k:v/1e6 for k,v in threads.items()},'inclusive':[{'name':k,'percent':100*v/total,'cpu_ms':v/1e6} for k,v in inc.most_common(200)],'self':[{'name':k,'percent':100*v/total,'cpu_ms':v/1e6} for k,v in own.most_common(100)]}
 (d/'cpu-summary.json').write_text(json.dumps(summary,indent=2));(d/'samples.json').write_text(json.dumps(samples))
 folded=collections.Counter()
 for s in main:folded[';'.join(reversed(s['frames']))]+=s['period']
 (d/'main.folded').write_text('\n'.join(f'{k} {v}' for k,v in folded.items())+'\n')
 print(name,len(main),'main samples',round(total/1e6,1),'CPU ms',flush=True)
