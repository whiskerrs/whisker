//! Drift detection between the in-tree `gen/` directory and the
//! current `Config` (+ a few related inputs).
//!
//! The fingerprint is a stable hex string written to
//! `gen/<platform>/.whisker-fingerprint`. On the next sync we recompute
//! it and compare. Equal → skip regeneration (fast path). Different →
//! regenerate.
//!
//! Fingerprinting the inputs rather than comparing mtimes: a
//! formatter touching `whisker.rs` would otherwise force a no-op
//! regeneration, and a hand-edit under `gen/` would survive unnoticed.
//!
//! FNV-1a is enough — the requirement is a stable u64 identical across
//! processes, not collision resistance.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// Compute the FNV-1a hash of `bytes` as a 16-hex-char string — the
/// form written to and compared against the on-disk fingerprint.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_16_hex_chars() {
        let fp = fingerprint(b"hello");
        assert_eq!(fp.len(), 16);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn same_input_produces_same_fingerprint() {
        assert_eq!(fingerprint(b"x"), fingerprint(b"x"));
    }

    #[test]
    fn different_inputs_produce_different_fingerprints() {
        assert_ne!(fingerprint(b"a"), fingerprint(b"b"));
    }

    #[test]
    fn empty_input_is_the_fnv_offset() {
        assert_eq!(fingerprint(b""), format!("{FNV_OFFSET:016x}"));
    }
}
