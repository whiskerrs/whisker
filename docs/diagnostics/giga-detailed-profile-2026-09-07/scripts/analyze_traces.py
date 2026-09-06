import subprocess,csv,io,json,sys,collections,statistics,bisect
from pathlib import Path
root=Path('/tmp/giga-detailed-profile');tp='/tmp/giga-android-frame-study/trace_processor'
def query(trace,sql):
 r=subprocess.run([tp,'query',str(trace),sql],text=True,capture_output=True,check=True)
 return list(csv.DictReader(io.StringIO(r.stdout)))
def quantile(v,q):
 v=sorted(v);return v[min(len(v)-1,int((len(v)-1)*q))] if v else 0
for d in sorted(root.glob('perfetto-*')):
 if not (d/'metadata.json').exists():continue
 meta=json.loads((d/'metadata.json').read_text());pid=meta['pid'];trace=d/'timeline.pftrace'
 raw=query(trace,f"SELECT s.id,s.ts,s.dur,s.name,s.parent_id,s.depth FROM slice s JOIN thread_track tt ON tt.id=s.track_id JOIN thread t USING(utid) WHERE t.tid={pid} AND s.dur>=0 ORDER BY s.ts")
 slices=[]
 for row in raw:
  row={k:(v if k=='name' else int(v) if v not in ['[NULL]','',None] else None) for k,v in row.items()};slices.append(row)
 states=query(trace,f"SELECT ts,dur,state FROM thread_state JOIN thread USING(utid) WHERE tid={pid} AND dur>0 ORDER BY ts")
 running=[(int(s['ts']),int(s['ts'])+int(s['dur'])) for s in states if s['state']=='Running']
 def run_ms(a,b):return sum(max(0,min(b,y)-max(a,x)) for x,y in running if x<b and y>a)/1e6
 children=collections.defaultdict(int)
 for s in slices:children[s['parent_id']]+=s['dur']
 scopes=[s for s in slices if s['name'].startswith('wk.') and s['ts']>=meta['action_start_ns']-20000000]
 groups=collections.defaultdict(list)
 for s in scopes:groups[s['name']].append(s)
 aggregate=[]
 for name,items in groups.items():
  vals=[s['dur']/1e6 for s in items]
  aggregate.append({'name':name,'count':len(items),'total_ms':sum(vals),'avg_ms':statistics.mean(vals),'p95_ms':quantile(vals,.95),'max_ms':max(vals),'self_ms':sum(max(0,s['dur']-children[s['id']]) for s in items)/1e6,'running_ms':sum(run_ms(s['ts'],s['ts']+s['dur']) for s in items)})
 aggregate.sort(key=lambda x:-x['total_ms'])
 (d/'scopes.json').write_text(json.dumps(aggregate,indent=2));(d/'slices.json').write_text(json.dumps(slices));(d/'thread-states.json').write_text(json.dumps(states))
 ticks=sorted(groups['wk.driver.tick'],key=lambda s:-s['dur'])[:10];worst=[]
 for tick in ticks:
  a=tick['ts'];b=a+tick['dur'];inside=collections.defaultdict(float)
  for s in scopes:
   if s['ts']>=a and s['ts']+s['dur']<=b:inside[s['name']]+=s['dur']/1e6
  worst.append({'id':tick['id'],'ts':a,'wall_ms':tick['dur']/1e6,'running_ms':run_ms(a,b),'scopes_ms':dict(inside)})
 (d/'worst-ticks.json').write_text(json.dumps(worst,indent=2))
 (d/'frame-timeline.json').write_text(json.dumps(query(trace,f"SELECT a.ts,a.dur,a.jank_type,a.present_type,a.layer_name FROM actual_frame_timeline_slice a JOIN process p USING(upid) WHERE p.pid={pid}"),indent=2))
 (d/'trace-stats.json').write_text(json.dumps(query(trace,"SELECT name,value,severity FROM stats WHERE value!=0 AND (severity='error' OR name GLOB '*lost*' OR name GLOB '*overrun*')"),indent=2))
 print(d.name, 'ticks',len(groups['wk.driver.tick']), 'top',[(r['name'],round(r['total_ms'],1)) for r in aggregate[:9]],flush=True)
