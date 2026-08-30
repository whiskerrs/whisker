//! Adapter between the raw C callback table and core's module Host API.

use std::ffi::c_void;
use std::rc::Rc;

use crate::value_codec::{RawValueArena, decode_value};
use whisker_driver_sys::{InvokeModuleCallback, ObserveModuleCallback, WhiskerValueRaw};
use whisker_runtime::module::{ModuleHost, ModuleResult};

extern "C" fn complete(data: *mut c_void, value: *const WhiskerValueRaw) {
    if data.is_null() {
        return;
    }
    let callback = unsafe { Box::from_raw(data.cast::<ModuleResult>()) };
    callback(unsafe { decode_value(value) });
}

/// Creates a platform-neutral module Host backed by a native callback table.
pub fn module_host(
    data: *mut c_void,
    invoke: InvokeModuleCallback,
    observe: ObserveModuleCallback,
) -> Rc<ModuleHost> {
    let data = data as usize;
    ModuleHost::new(
        move |module, method, args, is_async, result| {
            let mut arena = RawValueArena::default();
            let raw_args = args
                .iter()
                .map(|value| arena.encode(value))
                .collect::<Vec<_>>();
            let result_data = Box::into_raw(Box::new(result)).cast::<c_void>();
            let accepted = invoke(
                data as *mut c_void,
                module.as_ptr(),
                module.len(),
                method.as_ptr(),
                method.len(),
                if raw_args.is_empty() {
                    std::ptr::null()
                } else {
                    raw_args.as_ptr()
                },
                raw_args.len(),
                is_async,
                complete,
                result_data,
            );
            if !accepted {
                unsafe { drop(Box::from_raw(result_data.cast::<ModuleResult>())) };
            }
            accepted
        },
        move |module, event, observing| {
            observe(
                data as *mut c_void,
                module.as_ptr(),
                module.len(),
                event.as_ptr(),
                event.len(),
                observing,
            );
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_driver_sys::ModuleResultCallback;
    use whisker_runtime::module::{PlatformModule, with_module_host};
    use whisker_runtime::value::WhiskerValue;

    extern "C" fn echo(
        _data: *mut c_void,
        _module: *const u8,
        _module_len: usize,
        _method: *const u8,
        _method_len: usize,
        args: *const WhiskerValueRaw,
        count: usize,
        _is_async: bool,
        result: ModuleResultCallback,
        result_data: *mut c_void,
    ) -> bool {
        assert_eq!(count, 1);
        result(result_data, args);
        true
    }

    extern "C" fn observe(
        _data: *mut c_void,
        _module: *const u8,
        _module_len: usize,
        _event: *const u8,
        _event_len: usize,
        _observing: bool,
    ) {
    }

    #[test]
    fn ffi_adapter_preserves_typed_values_without_json() {
        let host = module_host(std::ptr::null_mut(), echo, observe);
        let result = with_module_host(&host, || {
            PlatformModule::named("demo").invoke("echo", vec![WhiskerValue::Int(42)])
        });
        assert_eq!(result, WhiskerValue::Int(42));
    }
}
