//! Executes a stub built by `build_stub_for_needed` on the host
//! architecture: the stub is linked into a shared library next to a C
//! caller, loaded into this process, and must land on a function of
//! this test binary.

// Linux only: on Mach-O the object writer prefixes the stub's symbol
// names a second time, so a stub never satisfies the reference it is for.
#![cfg(target_os = "linux")]

use std::ffi::{CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::Command;

use whisker_dev_server::hotpatch::{
    HotpatchModuleCache, build_stub_for_needed, linker_os_for_current_platform,
};

const TARGET: &str = "whisker_stub_roundtrip_target";
const RTLD_NOW: c_int = 2;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn whisker_stub_roundtrip_target() -> c_int {
    4242
}

fn work_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("whisker-stub-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stub_jumps_to_the_runtime_address_of_a_host_function() {
    let mut cache = HotpatchModuleCache::from_path(std::env::current_exe().unwrap()).unwrap();
    // The target stands in for the anchor: its static and runtime
    // addresses differ by exactly this process's load slide.
    cache.aslr_reference = cache.symbols.by_name[TARGET].address;
    let runtime_address = whisker_stub_roundtrip_target as *const () as usize as u64;

    let stub = build_stub_for_needed(
        &[TARGET.to_string()],
        &cache,
        linker_os_for_current_platform(),
        runtime_address,
    )
    .unwrap();

    let dir = work_dir();
    let stub_path = dir.join("stub.o");
    std::fs::write(&stub_path, stub).unwrap();
    let caller = dir.join("caller.c");
    std::fs::write(
        &caller,
        format!("int {TARGET}(void);\nint call_through_stub(void) {{ return {TARGET}(); }}\n"),
    )
    .unwrap();
    let library = dir.join("libstub_roundtrip.so");
    let status = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .arg("-shared")
        .arg("-fPIC")
        .arg(&caller)
        .arg(&stub_path)
        .arg("-o")
        .arg(&library)
        .status()
        .expect("spawn the C compiler");
    assert!(status.success(), "link the stub library");

    let path = CString::new(library.to_str().unwrap()).unwrap();
    let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
    assert!(!handle.is_null(), "load {}", library.display());
    let entry = unsafe { dlsym(handle, c"call_through_stub".as_ptr()) };
    assert!(!entry.is_null());
    let call: extern "C" fn() -> c_int = unsafe { std::mem::transmute(entry) };
    assert_eq!(call(), 4242);
    let _ = std::fs::remove_dir_all(&dir);
}
