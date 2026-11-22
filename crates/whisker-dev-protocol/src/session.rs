//! Replay-protected envelopes exchanged after the handshake.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{ENVELOPE_OVERHEAD, Error, MAX_CONTROL, MAX_MESSAGE, MAX_PAYLOAD, PROTOCOL_VERSION};

type HmacSha256 = Hmac<Sha256>;

const TAG_LEN: usize = 32;
const HEADER_LEN: usize = ENVELOPE_OVERHEAD - TAG_LEN;

/// What an envelope's payload contains
///
/// The kind is authenticated together with the payload, so a receiver can
/// route the payload without parsing it first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    /// A hot-reload patch: `[u64 BE header length][JSON jump table][library]`
    ///
    /// Only the server sends patches.
    Patch = 1,
    /// UTF-8 JSON control or metadata, at most [`MAX_CONTROL`] bytes
    ///
    /// Covers the client hello, logs, traces, and server control messages.
    Text = 2,
}

impl FrameKind {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Patch),
            2 => Some(Self::Text),
            _ => None,
        }
    }
}

/// Which end of the connection a transcript value belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Client,
    Server,
}

impl Role {
    fn peer(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }

    fn proof_domain(self) -> &'static [u8] {
        match self {
            Self::Client => b"client-proof",
            Self::Server => b"server-proof",
        }
    }

    fn frame_domain(self) -> &'static [u8] {
        match self {
            Self::Client => b"client-frame",
            Self::Server => b"server-frame",
        }
    }
}

fn transcript_mac(
    secret: &str,
    domain: &[u8],
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
) -> HmacSha256 {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(b"whisker-dev-v1\0");
    mac.update(domain);
    mac.update(client_nonce);
    mac.update(server_nonce);
    mac
}

pub(crate) fn proof(
    secret: &str,
    role: Role,
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
) -> [u8; 32] {
    transcript_mac(secret, role.proof_domain(), client_nonce, server_nonce)
        .finalize()
        .into_bytes()
        .into()
}

pub(crate) fn verify_proof(
    secret: &str,
    role: Role,
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
    tag: &[u8],
) -> bool {
    !secret.is_empty()
        && transcript_mac(secret, role.proof_domain(), client_nonce, server_nonce)
            .verify_slice(tag)
            .is_ok()
}

/// One authenticated connection's envelope state
///
/// A `Session` exists only after a successful handshake
/// ([`ClientHandshake::finish`](crate::ClientHandshake::finish) or
/// [`ServerHandshake::finish`](crate::ServerHandshake::finish)). It binds
/// every envelope to the shared secret, both handshake nonces, and the
/// sending direction, and keeps one sequence counter per direction.
///
/// [`seal`](Self::seal) and [`open`](Self::open) are the only way bytes
/// enter or leave the authenticated channel; anything [`open`](Self::open)
/// rejects must not be parsed, and the connection should be closed.
///
/// # Example
///
/// ```rust
/// use whisker_dev_protocol::{ClientHandshake, FrameKind, ServerHandshake};
///
/// let (client, challenge) = ClientHandshake::start("secret")?;
/// let (server, server_proof) = ServerHandshake::respond("secret", &challenge)?;
/// let (mut app, client_proof) = client.finish(&server_proof)?;
/// let mut host = server.finish(&client_proof)?;
///
/// let hello = app.seal(FrameKind::Text, br#"{"kind":"hello","aslr_reference":0}"#)?;
/// let (kind, payload) = host.open(&hello)?;
/// assert_eq!(kind, FrameKind::Text);
/// assert!(payload.starts_with(br#"{"kind":"hello""#));
///
/// // A host cannot accept its own envelope back (reflection).
/// let reflected = host.seal(FrameKind::Text, b"{}")?;
/// assert!(host.open(&reflected).is_err());
/// # Ok::<(), whisker_dev_protocol::Error>(())
/// ```
pub struct Session {
    secret: String,
    client_nonce: [u8; 32],
    server_nonce: [u8; 32],
    role: Role,
    sent: u64,
    received: u64,
}

impl Session {
    pub(crate) fn new(
        secret: String,
        client_nonce: [u8; 32],
        server_nonce: [u8; 32],
        role: Role,
    ) -> Self {
        Self {
            secret,
            client_nonce,
            server_nonce,
            role,
            sent: 0,
            received: 0,
        }
    }

    fn frame_mac(&self, sender: Role) -> HmacSha256 {
        transcript_mac(
            &self.secret,
            sender.frame_domain(),
            &self.client_nonce,
            &self.server_nonce,
        )
    }

    /// Wrap `payload` in an authenticated envelope for the peer
    ///
    /// Advances this direction's sequence number. Fails with
    /// [`Error::FrameTooLarge`] when the payload exceeds [`MAX_PAYLOAD`] (or
    /// [`MAX_CONTROL`] for [`FrameKind::Text`]), and with
    /// [`Error::SequenceExhausted`] instead of wrapping the counter.
    pub fn seal(&mut self, kind: FrameKind, payload: &[u8]) -> Result<Vec<u8>, Error> {
        if payload.len() > MAX_PAYLOAD || (kind == FrameKind::Text && payload.len() > MAX_CONTROL) {
            return Err(Error::FrameTooLarge);
        }
        let next = self.sent.checked_add(1).ok_or(Error::SequenceExhausted)?;
        let mut frame = Vec::with_capacity(payload.len() + ENVELOPE_OVERHEAD);
        frame.extend_from_slice(&[PROTOCOL_VERSION, kind as u8]);
        frame.extend_from_slice(&self.sent.to_be_bytes());
        frame.extend_from_slice(payload);
        let mut mac = self.frame_mac(self.role);
        mac.update(&frame);
        frame.extend_from_slice(&mac.finalize().into_bytes());
        self.sent = next;
        Ok(frame)
    }

    /// Verify an envelope from the peer and return its kind and payload
    ///
    /// The complete envelope is checked (size, version, kind, sequence, and
    /// tag) before any of the payload is returned, so callers never parse
    /// unauthenticated bytes. On success the receive sequence advances; on
    /// failure the session state is unchanged and the caller should close
    /// the connection.
    pub fn open<'a>(&mut self, frame: &'a [u8]) -> Result<(FrameKind, &'a [u8]), Error> {
        if frame.len() < ENVELOPE_OVERHEAD || frame[0] != PROTOCOL_VERSION {
            return Err(Error::InvalidFrame);
        }
        let kind = FrameKind::from_byte(frame[1]).ok_or(Error::InvalidFrame)?;
        let payload_len = frame.len() - ENVELOPE_OVERHEAD;
        if frame.len() > MAX_MESSAGE || (kind == FrameKind::Text && payload_len > MAX_CONTROL) {
            return Err(Error::FrameTooLarge);
        }
        let mut sequence = [0; 8];
        sequence.copy_from_slice(&frame[2..HEADER_LEN]);
        if u64::from_be_bytes(sequence) != self.received {
            return Err(Error::OutOfSequence);
        }
        let tag_start = frame.len() - TAG_LEN;
        let mut mac = self.frame_mac(self.role.peer());
        mac.update(&frame[..tag_start]);
        mac.verify_slice(&frame[tag_start..])
            .map_err(|_| Error::InvalidMac)?;
        self.received = self
            .received
            .checked_add(1)
            .ok_or(Error::SequenceExhausted)?;
        Ok((kind, &frame[HEADER_LEN..tag_start]))
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("role", &self.role)
            .field("sent", &self.sent)
            .field("received", &self.received)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Session, Session) {
        (
            Session::new("secret".into(), [1; 32], [2; 32], Role::Server),
            Session::new("secret".into(), [1; 32], [2; 32], Role::Client),
        )
    }

    #[test]
    fn proofs_bind_both_nonces_roles_and_secret() {
        let (client, server) = ([1; 32], [2; 32]);
        let tag = proof("secret", Role::Server, &client, &server);
        assert!(verify_proof("secret", Role::Server, &client, &server, &tag));
        assert!(!verify_proof("other", Role::Server, &client, &server, &tag));
        assert!(!verify_proof(
            "secret",
            Role::Client,
            &client,
            &server,
            &tag
        ));
        assert!(!verify_proof(
            "secret",
            Role::Server,
            &[3; 32],
            &server,
            &tag
        ));
        assert!(!verify_proof(
            "secret",
            Role::Server,
            &client,
            &[3; 32],
            &tag
        ));
        assert!(!verify_proof("", Role::Server, &client, &server, &tag));
    }

    #[test]
    fn envelopes_reject_tampering_reflection_replay_and_other_sessions() {
        let (mut sender, mut receiver) = pair();
        let frame = sender
            .seal(FrameKind::Patch, b"patch metadata and library")
            .unwrap();
        for index in [0, 1, 2, 10, frame.len() - 1] {
            let mut modified = frame.clone();
            modified[index] ^= 1;
            assert!(pair().1.open(&modified).is_err());
        }
        assert!(matches!(sender.open(&frame), Err(Error::InvalidMac)));
        let mut other = Session::new("secret".into(), [3; 32], [2; 32], Role::Client);
        assert!(matches!(other.open(&frame), Err(Error::InvalidMac)));
        assert_eq!(
            receiver.open(&frame).unwrap(),
            (FrameKind::Patch, b"patch metadata and library".as_slice())
        );
        assert!(matches!(receiver.open(&frame), Err(Error::OutOfSequence)));
        let next = sender.seal(FrameKind::Text, b"control").unwrap();
        assert_eq!(
            receiver.open(&next).unwrap(),
            (FrameKind::Text, b"control".as_slice())
        );
    }

    #[test]
    fn bounds_and_sequence_exhaustion_fail_closed() {
        let (mut sender, mut receiver) = pair();
        assert!(sender.seal(FrameKind::Text, &vec![0; MAX_CONTROL]).is_ok());
        assert!(matches!(
            sender.seal(FrameKind::Text, &vec![0; MAX_CONTROL + 1]),
            Err(Error::FrameTooLarge)
        ));
        assert!(matches!(
            receiver.open(&[0; ENVELOPE_OVERHEAD - 1]),
            Err(Error::InvalidFrame)
        ));
        let mut unknown_kind = vec![PROTOCOL_VERSION, 9];
        unknown_kind.resize(ENVELOPE_OVERHEAD, 0);
        assert!(matches!(
            receiver.open(&unknown_kind),
            Err(Error::InvalidFrame)
        ));
        let mut oversized_control = vec![PROTOCOL_VERSION, FrameKind::Text as u8];
        oversized_control.resize(ENVELOPE_OVERHEAD + MAX_CONTROL + 1, 0);
        assert!(matches!(
            receiver.open(&oversized_control),
            Err(Error::FrameTooLarge)
        ));
        sender.sent = u64::MAX;
        assert!(matches!(
            sender.seal(FrameKind::Patch, b"patch"),
            Err(Error::SequenceExhausted)
        ));
    }

    #[test]
    fn debug_output_omits_the_secret() {
        assert!(!format!("{:?}", pair().0).contains("secret"));
    }
}
