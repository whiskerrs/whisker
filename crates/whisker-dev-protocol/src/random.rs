//! Operating-system randomness for nonces, secrets, and file names.

/// Fill an `N`-byte array from the operating system's CSPRNG
///
/// Used for handshake nonces, the per-session secret, and unpredictable
/// staging-directory names. There is deliberately no fallback generator:
/// if the operating system cannot supply randomness, the caller fails
/// closed instead of continuing with a guessable value.
///
/// # Example
///
/// ```rust
/// let nonce: [u8; 32] = whisker_dev_protocol::random_bytes()?;
/// assert_eq!(nonce.len(), 32);
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn random_bytes<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut bytes = [0; N];
    fill(&mut bytes)?;
    Ok(bytes)
}

/// Generate a fresh per-session secret
///
/// The secret is 128 bits of operating-system randomness, hex-encoded into
/// 32 characters so it survives environment variables and Android system
/// properties unchanged. `whisker run` generates one per session and hands
/// it to the app out of band; it is never sent over the WebSocket.
///
/// # Example
///
/// ```rust
/// let secret = whisker_dev_protocol::generate_secret()?;
/// assert_eq!(secret.len(), 32);
/// assert!(secret.bytes().all(|byte| byte.is_ascii_hexdigit()));
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn generate_secret() -> std::io::Result<String> {
    Ok(random_bytes::<16>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(not(windows))]
fn fill(bytes: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(bytes)
}

#[cfg(windows)]
fn fill(bytes: &mut [u8]) -> std::io::Result<()> {
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    let length =
        u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("random buffer too large"))?;
    // SAFETY: a null algorithm handle is valid with
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG, and the call writes at most `length`
    // bytes into `bytes`, which is exactly that long.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(std::io::Error::other("BCryptGenRandom failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn consecutive_values_differ() {
        assert_ne!(
            super::random_bytes::<32>().unwrap(),
            super::random_bytes::<32>().unwrap()
        );
        assert_ne!(
            super::generate_secret().unwrap(),
            super::generate_secret().unwrap()
        );
    }
}
