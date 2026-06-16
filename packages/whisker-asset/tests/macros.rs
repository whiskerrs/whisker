//! Integration test: actually invoke the asset macros against fixture
//! files under `packages/whisker-asset/assets/`. Because this test crate's
//! `CARGO_MANIFEST_DIR` is `packages/whisker-asset`, the macros' existence
//! check + `include_*!` resolve against that crate's `assets/` dir.

use whisker_asset::{asset, asset_bytes, asset_str};

#[test]
fn asset_str_embeds_fixture_content() {
    let content: &'static str = asset_str!("fixtures/sample.txt");
    assert_eq!(content, "sample-asset-content\n");
}

#[test]
fn asset_bytes_embeds_fixture_content() {
    let bytes: &'static [u8] = asset_bytes!("fixtures/sample.bin");
    assert_eq!(bytes, &[0x00, 0x01, 0x02, 0x03, 0xde, 0xad, 0xbe, 0xef]);
}

#[test]
fn asset_resolves_to_relative_path_when_no_base() {
    // No base installed in this test process → fallback returns the
    // (normalized) logical path unchanged.
    let resolved: String = asset!("fixtures/sample.txt");
    assert_eq!(resolved, "fixtures/sample.txt");
}
