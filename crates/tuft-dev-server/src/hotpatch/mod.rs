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

pub mod cache;
pub mod jump_table;
pub mod symbol_table;
pub mod thin_build;
pub mod wrapper;

pub use cache::HotpatchModuleCache;
pub use jump_table::{build_jump_table, DiffReport, PatchPlan};
pub use symbol_table::{parse_symbol_table, SymbolInfo, SymbolTable};
pub use thin_build::{build_thin_rebuild_plan, ThinRebuildPlan};
pub use wrapper::{
    default_cache_dir, load_captured_args, run_fat_build, CapturedRustcInvocation,
};
