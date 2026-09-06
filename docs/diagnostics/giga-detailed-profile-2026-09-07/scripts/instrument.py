from pathlib import Path
import shutil
root=Path('/tmp/giga-detailed-profile'); reg=Path('/Users/itome/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f'); work=Path('/tmp/whisker-list-frame-performance')
crates=['whisker-runtime','whisker-engine','whisker-driver']
helper='''// Temporary Android profiling instrumentation; not a product API.
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ATrace_beginSection(name: *const core::ffi::c_char);
    fn ATrace_endSection();
    fn ATrace_isEnabled() -> bool;
}
pub(crate) struct Scope(bool);
pub(crate) fn scope(name: &'static core::ffi::CStr) -> Scope {
    #[cfg(target_os = "android")]
    unsafe {
        let enabled = ATrace_isEnabled();
        if enabled { ATrace_beginSection(name.as_ptr()); }
        Scope(enabled)
    }
    #[cfg(not(target_os = "android"))]
    { let _ = name; Scope(false) }
}
impl Drop for Scope {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        if self.0 { unsafe { ATrace_endSection() } }
    }
}
'''
bridge=root/'codex-android-profile';(bridge/'src').mkdir(parents=True,exist_ok=True)
(bridge/'Cargo.toml').write_text('[package]\nname = "codex-android-profile"\nversion = "0.1.0"\nedition = "2024"\n')
(bridge/'src/lib.rs').write_text((root/'trace-helper.rs').read_text() if (root/'trace-helper.rs').exists() else helper.replace('pub(crate)', 'pub').replace('pub struct Scope(bool);', '#[allow(dead_code)]\npub struct Scope(bool);'))
for c in crates:
 dest=root/c
 if dest.exists():shutil.rmtree(dest)
 shutil.copytree(reg/f'{c}-0.13.7',dest)
 (dest/'src/codex_profile.rs').write_text('pub(crate) use codex_android_profile::scope;\n')
 p=dest/'Cargo.toml';p.write_text(p.read_text()+f'\n[dependencies.codex-android-profile]\npath = "{bridge}"\n')
 p=dest/'src/lib.rs';p.write_text(p.read_text()+'\nmod codex_profile;\n')
for rel in ['src/surface_runtime.rs','src/surface_runtime/renderer.rs']:
 shutil.copyfile(work/'crates/whisker-runtime'/rel,root/'whisker-runtime'/rel)
def fn(c,path,name,label):
 p=root/c/'src'/path;s=p.read_text();start=s.index('fn '+name);body=s.index('{',start)
 s=s[:body+1]+f'\n        let _profile = crate::codex_profile::scope(c"wk.{label}");'+s[body+1:];p.write_text(s)
def replace(c,path,a,b):
 p=root/c/'src'/path;s=p.read_text()
 if path=='surface_runtime.rs' and ('self.surface.clone()' in a or 'reapply_active_transitions' in a):
  start=s.index('fn apply_subtrees_now');prefix=s[:start];tail=s[start:];assert tail.count(a)==1; s=prefix+tail.replace(a,b)
 else:
  assert s.count(a)==1,(path,a,s.count(a));s=s.replace(a,b)
 p.write_text(s)
for name,label in [('drive_layout','runtime.layout'),('present<','runtime.present'),('apply_subtrees_now','style.apply'),('style_changes_inheritance','style.check_inheritance'),('motion_snapshots','style.motion_capture')]:fn('whisker-runtime','surface_runtime.rs',name,label)
for name,label in [('drive_frame<','runtime.frame'),('dispatch_input_active','runtime.input'),('drain_pending_host_events','runtime.host_events')]:fn('whisker-runtime','runtime_instance.rs',name,label)
for name,label in [('new_mounted_entry','list.mount_row'),('set_spacer_size','list.spacer'),('set_scroll_extent_size','list.extent'),('reconcile_grid_window','list.grid_reconcile'),('update_measurements','list.measurements'),('replace(','list.replace_items'),('sync_child_order','list.child_order')]:fn('whisker-runtime','view/virtualizer.rs',name,label)
fn('whisker-runtime','reactive/scheduler.rs','flush(','reactive.flush')
fn('whisker-runtime','tasks.rs','run_until_stalled','tasks.poll')
fn('whisker-runtime','surface_runtime/motion.rs','step_motion','motion.step')
replace('whisker-runtime','surface_runtime.rs','let mut surface = self.surface.clone();\n        let mut background_resources = self.background_resources.clone();','let (mut surface, mut background_resources) = {\n            let _profile = crate::codex_profile::scope(c"wk.style.clone");\n            (self.surface.clone(), self.background_resources.clone())\n        };')
replace('whisker-runtime','surface_runtime.rs','        for element in &roots {','        let prepare_profile = crate::codex_profile::scope(c"wk.style.prepare");\n        for element in &roots {')
replace('whisker-runtime','surface_runtime.rs','        Self::reapply_active_transitions(&self.elements, &mut surface)?;','        drop(prepare_profile);\n        Self::reapply_active_transitions(&self.elements, &mut surface)?;')
replace('whisker-runtime','surface_runtime.rs','            self.begin_mutation_batch();\n            with_installed_renderer','            let _profile = crate::codex_profile::scope(c"wk.layout.notifications");\n            self.begin_mutation_batch();\n            with_installed_renderer')
for name,label in [('drive_layout','engine.layout')]:fn('whisker-engine','layout.rs',name,label)
for name,label in [('compute_layout_with_measurements','layout.compute'),('project_layout','layout.project'),('prepare_frame','frame.prepare'),('present<','engine.present')]:fn('whisker-engine','surface.rs',name,label)
replace('whisker-engine','surface.rs','        let snapshot = self\n            .layout\n            .compute(root, viewport, &mut self.measurements)?;','        let snapshot = {\n            let _profile = crate::codex_profile::scope(c"wk.layout.taffy");\n            self.layout.compute(root, viewport, &mut self.measurements)?\n        };')
fn('whisker-driver','ffi_runtime.rs','tick(','driver.tick')
fn('whisker-driver','ffi_runtime/measurement.rs','measure_batch','host.measure_batch')
fn('whisker-driver','ffi_runtime/frame.rs','present(','host.present')
fn('whisker-driver','ffi_runtime/frame.rs','new(packet:','host.encode_frame')
replace('whisker-driver','ffi_runtime/frame.rs','        if !(self.present)(self.data, &owned.value, &mut response) {','        let host_profile = crate::codex_profile::scope(c"wk.host.android_apply");\n        let accepted = (self.present)(self.data, &owned.value, &mut response);\n        drop(host_profile);\n        if !accepted {')
print('Instrumented runtime/engine/driver with ATrace scopes')
