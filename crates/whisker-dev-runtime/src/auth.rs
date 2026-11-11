//! Versioned mutual authentication and replay-protected development frames.

use hmac::{Hmac, Mac};
use sha2::Sha256;
#[cfg(not(windows))]
use std::io::Read;

type HmacSha256 = Hmac<Sha256>;

/// Maximum authenticated WebSocket message size, including its envelope
pub const MAX_MESSAGE: usize = 256 * 1024 * 1024;
/// Maximum control payload and patch metadata size
pub const MAX_CONTROL: usize = 1024 * 1024;
/// Maximum handshake JSON size
pub const MAX_HANDSHAKE: usize = 4096;
/// Patch payload frame type
pub const PATCH: u8 = 1;
/// UTF-8 control and client metadata frame type
pub const TEXT: u8 = 2;

/// Initial client challenge; no shared secret crosses the wire
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub version: u8,
    pub nonce: [u8; 32],
}

/// Server nonce and proof of knowledge of the shared session secret
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerProof {
    pub nonce: [u8; 32],
    pub mac: [u8; 32],
}

/// Client proof bound to both fresh nonces
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientProof {
    pub mac: [u8; 32],
}

/// Obtain a fresh challenge from the operating system, failing closed on error
pub fn nonce() -> std::io::Result<[u8; 32]> {
    let mut bytes = [0; 32];
    #[cfg(not(windows))]
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                length: u32,
                flags: u32,
            ) -> i32;
        }
        // SAFETY: the system RNG accepts a null algorithm with flag 2 and writes at most the supplied buffer length.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                2,
            )
        };
        if status < 0 {
            return Err(std::io::Error::other("OS random generator failed"));
        }
    }
    Ok(bytes)
}

fn mac(
    token: &str,
    domain: &[u8],
    client: &[u8; 32],
    server: &[u8; 32],
    payload: &[u8],
) -> HmacSha256 {
    let mut mac =
        HmacSha256::new_from_slice(token.as_bytes()).expect("HMAC accepts any key length");
    mac.update(b"whisker-dev-v1\0");
    mac.update(domain);
    mac.update(client);
    mac.update(server);
    mac.update(payload);
    mac
}

/// Compute a directional handshake proof over both nonce challenges
pub fn proof(token: &str, server_side: bool, client: &[u8; 32], server: &[u8; 32]) -> [u8; 32] {
    mac(
        token,
        if server_side {
            b"server-proof"
        } else {
            b"client-proof"
        },
        client,
        server,
        &[],
    )
    .finalize()
    .into_bytes()
    .into()
}

/// Verify a directional handshake proof using the MAC implementation's constant-time comparison
pub fn verify_proof(
    token: &str,
    server_side: bool,
    client: &[u8; 32],
    server: &[u8; 32],
    tag: &[u8],
) -> bool {
    !token.is_empty()
        && mac(
            token,
            if server_side {
                b"server-proof"
            } else {
                b"client-proof"
            },
            client,
            server,
            &[],
        )
        .verify_slice(tag)
        .is_ok()
}

/// Per-connection directional sequence counters and authenticated transcript
pub struct Session {
    token: String,
    client: [u8; 32],
    server: [u8; 32],
    server_side: bool,
    sent: u64,
    received: u64,
}

impl Session {
    /// Construct a session only after verifying the peer's handshake proof
    pub fn new(token: String, client: [u8; 32], server: [u8; 32], server_side: bool) -> Self {
        Self {
            token,
            client,
            server,
            server_side,
            sent: 0,
            received: 0,
        }
    }

    fn frame_mac(&self, server_side: bool, frame: &[u8]) -> HmacSha256 {
        mac(
            &self.token,
            if server_side {
                b"server-frame"
            } else {
                b"client-frame"
            },
            &self.client,
            &self.server,
            frame,
        )
    }

    /// Authenticate a typed payload and advance the send sequence without wrapping
    pub fn seal(&mut self, kind: u8, payload: &[u8]) -> Result<Vec<u8>, &'static str> {
        if self.token.is_empty()
            || !matches!(kind, PATCH | TEXT)
            || payload.len() > MAX_MESSAGE - 42
            || (kind == TEXT && payload.len() > MAX_CONTROL)
        {
            return Err("invalid development frame size or type");
        }
        let next = self
            .sent
            .checked_add(1)
            .ok_or("development sequence exhausted")?;
        let mut frame = Vec::with_capacity(payload.len() + 42);
        frame.extend_from_slice(&[1, kind]);
        frame.extend_from_slice(&self.sent.to_be_bytes());
        frame.extend_from_slice(payload);
        let tag = self
            .frame_mac(self.server_side, &frame)
            .finalize()
            .into_bytes();
        frame.extend_from_slice(&tag);
        self.sent = next;
        Ok(frame)
    }

    /// Verify the complete envelope before exposing bytes to parsers or executable staging
    pub fn open<'a>(&mut self, frame: &'a [u8]) -> Result<(u8, &'a [u8]), &'static str> {
        if self.token.is_empty()
            || frame.len() < 42
            || frame.len() > MAX_MESSAGE
            || frame[0] != 1
            || !matches!(frame[1], PATCH | TEXT)
        {
            return Err("invalid development frame");
        }
        if frame[1] == TEXT && frame.len() - 42 > MAX_CONTROL {
            return Err("development control too large");
        }
        let seq = u64::from_be_bytes(frame[2..10].try_into().map_err(|_| "invalid sequence")?);
        if seq != self.received {
            return Err("replayed or out-of-order development frame");
        }
        let end = frame.len() - 32;
        self.frame_mac(!self.server_side, &frame[..end])
            .verify_slice(&frame[end..])
            .map_err(|_| "invalid development frame MAC")?;
        self.received = self
            .received
            .checked_add(1)
            .ok_or("development sequence exhausted")?;
        Ok((frame[1], &frame[10..end]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofs_bind_both_nonces_roles_and_token() {
        let client = [1; 32];
        let server = [2; 32];
        let tag = proof("secret", true, &client, &server);
        assert!(verify_proof("secret", true, &client, &server, &tag));
        assert!(!verify_proof("other", true, &client, &server, &tag));
        assert!(!verify_proof("secret", false, &client, &server, &tag));
        assert!(!verify_proof("secret", true, &[3; 32], &server, &tag));
        assert!(!verify_proof("secret", true, &client, &[3; 32], &tag));
        assert!(!verify_proof("", true, &client, &server, &tag));
    }

    #[test]
    fn frames_reject_tampering_reflection_replay_and_other_sessions() {
        let mut sender = Session::new("secret".into(), [1; 32], [2; 32], true);
        let frame = sender.seal(PATCH, b"patch metadata and library").unwrap();
        for index in [0, 1, 2, 10, frame.len() - 1] {
            let mut modified = frame.clone();
            modified[index] ^= 1;
            let mut receiver = Session::new("secret".into(), [1; 32], [2; 32], false);
            assert!(receiver.open(&modified).is_err());
        }
        assert!(sender.open(&frame).is_err());
        let mut other = Session::new("secret".into(), [3; 32], [2; 32], false);
        assert!(other.open(&frame).is_err());
        let mut receiver = Session::new("secret".into(), [1; 32], [2; 32], false);
        assert_eq!(
            receiver.open(&frame).unwrap(),
            (PATCH, b"patch metadata and library".as_slice())
        );
        assert!(receiver.open(&frame).is_err());
        let next = sender.seal(TEXT, b"control").unwrap();
        assert_eq!(receiver.open(&next).unwrap(), (TEXT, b"control".as_slice()));
    }

    #[test]
    fn bounds_and_sequence_exhaustion_fail_closed() {
        let mut session = Session::new("secret".into(), [1; 32], [2; 32], true);
        assert!(session.seal(TEXT, &vec![0; MAX_CONTROL]).is_ok());
        assert!(session.seal(TEXT, &vec![0; MAX_CONTROL + 1]).is_err());
        assert!(session.open(&[0; 41]).is_err());
        session.sent = u64::MAX;
        assert!(session.seal(PATCH, b"patch").is_err());
    }
}
