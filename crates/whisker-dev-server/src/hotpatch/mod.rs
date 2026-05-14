//! Tier 1 (subsecond) hot-reload pipeline.
//!
//! See `docs/hot-reload-plan.md` for the architecture. The modules
//! land one per task ID:
//!
//! - [`symbol_table`] (I4g-1): parse ELF / Mach-O symbol tables
//! - `jump_table`     (I4g-2): diff old vs new symbol tables
//! - `cache`          (I4g-3): hold the original module's parsed table
//! - `wrapper`        (I4g-4): rustc + linker hijack
//! - `thin_build`     (I4g-5): partial rebuild driver
//! - `Patcher`        (I4g-6): the integrated `Change → JumpTable` API

pub mod android_ndk;
pub mod cache;
pub mod jump_table;
pub mod link_plan;
pub mod patcher;
pub mod runner;
pub mod shim_paths;
pub mod stub_object;
pub mod symbol_table;
pub mod thin_build;
pub mod validate;
pub mod wrapper;

pub use cache::HotpatchModuleCache;
pub use jump_table::{build_jump_table, DiffReport, PatchPlan};
pub use link_plan::{build_link_plan, linker_os_for_host, LinkPlan, LinkerOs};
pub use patcher::Patcher;
pub use runner::{run_link_plan, run_obj_plan, thin_rebuild_obj};
pub use shim_paths::{expected_shim_paths, resolve_shim_paths, ShimPaths};
pub use stub_object::create_undefined_symbol_stub;
pub use symbol_table::{parse_symbol_table, SymbolInfo, SymbolTable};
pub use thin_build::{build_obj_plan, library_filename, object_filename, ObjBuildPlan};
pub use validate::{ensure_target_supported, extract_target_triple, validate_environment};
pub use wrapper::{
    default_cache_dir, default_linker_cache_dir, load_captured_args, load_captured_linker_args,
    resolve_host_linker, run_fat_build, CapturedLinkerInvocation, CapturedRustcInvocation,
    LinkerCaptureConfig,
};
