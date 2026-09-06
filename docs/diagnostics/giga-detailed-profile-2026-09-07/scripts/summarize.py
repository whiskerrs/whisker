from pathlib import Path
import json,collections
root=Path('/tmp/giga-detailed-profile');result={'build':'debug, bb5656bd runtime optimizations + temporary ATrace instrumentation','device':'emulator-5554 / sdk_gphone16k_arm64 / API 37 / 1080x2400','binary_identity':json.loads((root/'binary-identity.json').read_text()),'perfetto':{},'simpleperf':{}}
for d in sorted(root.glob('perfetto-*')):
 meta=json.loads((d/'metadata.json').read_text());ticks=json.loads((d/'worst-ticks.json').read_text());slices=json.loads((d/'slices.json').read_text());w=ticks[0];a=w['ts'];b=a+round(w['wall_ms']*1e6)
 w['counts']=dict(collections.Counter(s['name'] for s in slices if s['ts']>=a and s['ts']+s['dur']<=b+2 and s['name'].startswith('wk.')))
 frames=json.loads((d/'frame-timeline.json').read_text());matches=[f for f in frames if int(f['ts'])<=a and int(f['ts'])+int(f['dur'])>=b-2]
 w['containing_frame']=min(matches,key=lambda f:int(f['dur'])) if matches else None
 result['perfetto'][d.name]={'metadata':meta,'scopes':json.loads((d/'scopes.json').read_text()),'worst_tick':w,'trace_errors':json.loads((d/'trace-stats.json').read_text())}
for d in sorted(root.glob('simpleperf-*')):
 meta=json.loads((d/'cpu-summary.json').read_text());samples=[s for s in json.loads((d/'samples.json').read_text()) if s['tid']==meta['main_tid']];total=sum(s['period'] for s in samples)
 preds={'layout':lambda f:'whisker_layout::LayoutTree::compute::' in f,'surface_clone':lambda f:'SurfaceEngine' in f and 'as$u20$core..clone..Clone$GT$::clone::' in f,'surface_drop':lambda f:'drop_in_place$LT$whisker_engine..surface..SurfaceEngine$GT$' in f,'host_commit':lambda f:'rs.whisker.runtime.scene.HostScene.commit ' in f,'measurement_apply':lambda f:'MeasurementCoordinator::apply_batch::' in f,'row_mount':lambda f:'virtualizer::new_mounted_entry::h' in f}
 result['simpleperf'][d.name]={'main_samples':meta['main_samples'],'all_samples':meta['all_samples'],'sampled_main_cpu_ms':meta['sampled_main_cpu_ms'],'categories_percent':{k:100*sum(s['period'] for s in samples if any(pred(f) for f in s['frames']))/total for k,pred in preds.items()}}
result['profiler_disabled_control']={d.name:json.loads((d/'metadata.json').read_text()) for d in root.glob('control-*') if d.is_dir()}
(root/'summary.json').write_text(json.dumps(result,indent=2,ensure_ascii=False))
