//! Authenticated wire protocol for the `whisker run` development channel
//!
//! `whisker run` (through `whisker-dev-server`) and the app it launches
//! (through `whisker-dev-runtime`) talk over a WebSocket that can deliver
//! native libraries the app then loads. This crate holds everything both
//! sides must agree on so that only the app launched by the current session
//! accepts those libraries, and only the current session's server can send
//! them:
//!
//! - the per-session secret ([`generate_secret`]) and operating-system
//!   randomness ([`random_bytes`]),
//! - the mutual-authentication handshake ([`ClientHandshake`] and
//!   [`ServerHandshake`]),
//! - the replay-protected envelope every later message travels in
//!   ([`Session`] and [`FrameKind`]),
//! - the size limits and timeout both peers enforce.
//!
//! The crate performs no I/O. The dev server and the dev runtime own their
//! sockets, feed received handshake text into these state machines, and send
//! whatever text or bytes the state machines return.
//!
//! # Handshake
//!
//! ```text
//! app (client)                               whisker run (server)
//!   ClientHandshake::start ── challenge ──▶   ServerHandshake::respond
//!   ClientHandshake::finish ◀── server proof ──
//!                            ── client proof ─▶ ServerHandshake::finish
//! ```
//!
//! Both nonces are fresh 256-bit random values. Each proof is an
//! HMAC-SHA256 over both nonces under a role-specific domain, so neither
//! proof can be reflected back as the other. The secret itself never crosses
//! the wire. The client verifies the server before it sends anything that
//! identifies the process, which keeps a substituted server from learning the
//! app's ASLR reference, logs, or trace data. There is no version
//! negotiation: a peer built against a different protocol version fails the
//! handshake with an [`Error`] that names the mismatch.
//!
//! # Envelope
//!
//! After the handshake every message is a binary envelope:
//!
//! ```text
//! [version: u8][kind: u8][sequence: u64 BE][payload][HMAC-SHA256: 32 bytes]
//! ```
//!
//! The tag covers the direction, both nonces, and every envelope byte before
//! it. Each direction must use the next sequence number, so modified,
//! replayed, reordered, reflected, and cross-session envelopes fail
//! [`Session::open`] before the payload reaches a parser.
//!
//! # Example
//!
//! ```rust
//! use whisker_dev_protocol::{ClientHandshake, FrameKind, ServerHandshake};
//!
//! let secret = whisker_dev_protocol::generate_secret()?;
//!
//! let (client, challenge) = ClientHandshake::start(&secret)?;
//! let (server, server_proof) = ServerHandshake::respond(&secret, &challenge)?;
//! let (mut app, client_proof) = client.finish(&server_proof)?;
//! let mut host = server.finish(&client_proof)?;
//!
//! let envelope = host.seal(FrameKind::Text, br#"{"kind":"reload"}"#)?;
//! assert_eq!(
//!     app.open(&envelope)?,
//!     (FrameKind::Text, br#"{"kind":"reload"}"#.as_slice())
//! );
//! // The same envelope is rejected the second time.
//! assert!(app.open(&envelope).is_err());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Trust boundary
//!
//! HMAC authenticates the channel but does not encrypt it. The operating
//! system's debug transport (environment variables, `adb`) that delivers the
//! secret to the app is trusted.

#![warn(missing_docs)]

mod handshake;
mod random;
mod session;

use std::time::Duration;

pub use handshake::{ClientHandshake, ServerHandshake};
pub use random::{generate_secret, random_bytes};
pub use session::{FrameKind, Session};

/// Version byte carried by the challenge and every envelope
///
/// Both peers must be built from the same protocol version; a mismatch
/// fails the handshake with [`Error::UnsupportedVersion`] rather than
/// falling back to an older scheme.
pub const PROTOCOL_VERSION: u8 = 1;

/// Largest complete envelope, header and tag included, in bytes
///
/// This bounds a single hot-reload patch. A patch that does not fit needs a
/// Full Reload instead.
pub const MAX_MESSAGE: usize = 256 * 1024 * 1024;

/// Bytes an envelope adds around its payload
///
/// One version byte, one kind byte, an eight-byte sequence number, and a
/// 32-byte HMAC-SHA256 tag.
pub const ENVELOPE_OVERHEAD: usize = 1 + 1 + 8 + 32;

/// Largest payload a single envelope carries, in bytes
pub const MAX_PAYLOAD: usize = MAX_MESSAGE - ENVELOPE_OVERHEAD;

/// Largest [`FrameKind::Text`] payload and patch metadata header, in bytes
///
/// Control messages, logs, traces, and the JSON jump table that prefixes a
/// patch are all far smaller than this. Servers use it, plus
/// [`ENVELOPE_OVERHEAD`], as their read limit because clients never send
/// patches.
pub const MAX_CONTROL: usize = 1024 * 1024;

/// Largest handshake message, in bytes
///
/// Handshake messages are a few hundred bytes of JSON. Receivers check this
/// before parsing; the dev runtime additionally caps how many bytes it reads
/// at all until the handshake completes.
pub const MAX_HANDSHAKE: usize = 4096;

/// Time either peer allows for the complete handshake
///
/// A peer that has not authenticated within this window is disconnected,
/// so a stalled connection cannot hold a session slot open.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why a handshake or envelope was rejected
///
/// Every variant is terminal for the connection: the caller closes the
/// socket and, for handshake failures, reports the message so a developer
/// can tell a stale app or a version mismatch apart from a missing server.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The operating system could not supply random bytes
    #[error("operating-system randomness is unavailable: {0}")]
    Random(#[from] std::io::Error),
    /// No session secret was provisioned
    #[error("no development session secret was provisioned")]
    MissingSecret,
    /// A handshake message exceeded [`MAX_HANDSHAKE`]
    #[error("handshake message exceeds {MAX_HANDSHAKE} bytes")]
    HandshakeTooLarge,
    /// A handshake message was not valid for this protocol version
    #[error(
        "malformed handshake message ({0}); rebuild the app with the same Whisker version as the CLI"
    )]
    MalformedHandshake(#[source] serde_json::Error),
    /// The peer announced a different [`PROTOCOL_VERSION`]
    #[error(
        "peer speaks development protocol version {0}, expected {PROTOCOL_VERSION}; rebuild the app with the same Whisker version as the CLI"
    )]
    UnsupportedVersion(u8),
    /// The server could not prove it holds this app's session secret
    #[error(
        "the development server does not hold this app's session secret (the app was launched by a different `whisker run` session)"
    )]
    InvalidServerProof,
    /// The client could not prove it holds this server's session secret
    #[error(
        "the app does not hold this session's secret (it was launched by a different `whisker run` session)"
    )]
    InvalidClientProof,
    /// An envelope had an unknown version or kind, or was truncated
    #[error("malformed development frame")]
    InvalidFrame,
    /// An envelope or its payload exceeded the size limits
    #[error("development frame exceeds the size limit")]
    FrameTooLarge,
    /// An envelope did not carry the next expected sequence number
    #[error("replayed or out-of-order development frame")]
    OutOfSequence,
    /// An envelope's tag did not verify
    #[error("development frame failed authentication")]
    InvalidMac,
    /// A direction used every available sequence number
    #[error("development frame sequence exhausted")]
    SequenceExhausted,
}
