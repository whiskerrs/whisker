#[cfg(target_os = "android")]
mod android {
    use core::ffi::{c_char,c_void,CStr};
    use std::sync::OnceLock;
    unsafe extern "C" {
        fn dlopen(name: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    struct TraceFns {
        begin: unsafe extern "C" fn(*const c_char),
        end: unsafe extern "C" fn(),
        enabled: unsafe extern "C" fn() -> bool,
    }
    static FNS: OnceLock<Option<TraceFns>> = OnceLock::new();
    fn functions() -> Option<&'static TraceFns> {
        FNS.get_or_init(|| unsafe {
            let handle = dlopen(c"libandroid.so".as_ptr(), 2);
            if handle.is_null() { return None; }
            let begin=dlsym(handle,c"ATrace_beginSection".as_ptr());
            let end=dlsym(handle,c"ATrace_endSection".as_ptr());
            let enabled=dlsym(handle,c"ATrace_isEnabled".as_ptr());
            if begin.is_null() || end.is_null() || enabled.is_null() { return None; }
            Some(TraceFns {begin:core::mem::transmute::<*mut c_void,unsafe extern "C" fn(*const c_char)>(begin),end:core::mem::transmute::<*mut c_void,unsafe extern "C" fn()>(end),enabled:core::mem::transmute::<*mut c_void,unsafe extern "C" fn()->bool>(enabled)})
        }).as_ref()
    }
    pub struct Scope(bool);
    pub fn scope(name: &'static CStr) -> Scope {
        let active=functions().is_some_and(|f| unsafe {
            if (f.enabled)() { (f.begin)(name.as_ptr());true } else {false}
        });
        Scope(active)
    }
    impl Drop for Scope { fn drop(&mut self) { if self.0 { unsafe {(functions().unwrap().end)()} } } }
}
#[cfg(target_os = "android")]
pub use android::{scope,Scope};
#[cfg(not(target_os = "android"))]
pub struct Scope;
#[cfg(not(target_os = "android"))]
pub fn scope(_: &'static core::ffi::CStr) -> Scope { Scope }
