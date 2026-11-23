//! Mutual-authentication handshake state machines.

use serde::{Deserialize, Serialize};

use crate::session::{Role, Session, proof, verify_proof};
use crate::{Error, MAX_HANDSHAKE, PROTOCOL_VERSION, random_bytes};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    version: u8,
    nonce: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerProof {
    nonce: [u8; 32],
    mac: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientProof {
    mac: [u8; 32],
}

fn parse<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, Error> {
    if text.len() > MAX_HANDSHAKE {
        return Err(Error::HandshakeTooLarge);
    }
    serde_json::from_str(text).map_err(Error::MalformedHandshake)
}

fn encode<T: Serialize>(message: &T) -> Result<String, Error> {
    serde_json::to_string(message).map_err(Error::MalformedHandshake)
}

/// Client (app) side of the handshake, between the challenge and the server proof
///
/// The dev runtime calls [`start`](Self::start), sends the returned
/// challenge as a WebSocket text message, waits for the server's text reply,
/// and passes it to [`finish`](Self::finish). Only after `finish` succeeds
/// does the app send its proof and then anything identifying the process.
///
/// # Example
///
/// ```rust
/// use whisker_dev_protocol::{ClientHandshake, Error, ServerHandshake};
///
/// let (client, challenge) = ClientHandshake::start("app secret")?;
/// // A server from another session holds a different secret.
/// let (_server, proof) = ServerHandshake::respond("other secret", &challenge)?;
/// assert!(matches!(client.finish(&proof), Err(Error::InvalidServerProof)));
/// # Ok::<(), Error>(())
/// ```
pub struct ClientHandshake {
    secret: String,
    nonce: [u8; 32],
}

impl ClientHandshake {
    /// Begin a handshake and return the challenge text to send
    ///
    /// Fails with [`Error::MissingSecret`] for an empty secret and with
    /// [`Error::Random`] if no nonce can be generated.
    pub fn start(secret: &str) -> Result<(Self, String), Error> {
        if secret.is_empty() {
            return Err(Error::MissingSecret);
        }
        let nonce = random_bytes()?;
        let challenge = encode(&Challenge {
            version: PROTOCOL_VERSION,
            nonce,
        })?;
        Ok((
            Self {
                secret: secret.to_owned(),
                nonce,
            },
            challenge,
        ))
    }

    /// Verify the server's proof and return the session plus the client proof to send
    ///
    /// Fails with [`Error::InvalidServerProof`] when the server does not hold
    /// the same secret, which is what an app left over from a previous
    /// `whisker run` session observes.
    pub fn finish(self, server_proof: &str) -> Result<(Session, String), Error> {
        let server: ServerProof = parse(server_proof)?;
        if !verify_proof(
            &self.secret,
            Role::Server,
            &self.nonce,
            &server.nonce,
            &server.mac,
        ) {
            return Err(Error::InvalidServerProof);
        }
        let reply = encode(&ClientProof {
            mac: proof(&self.secret, Role::Client, &self.nonce, &server.nonce),
        })?;
        Ok((
            Session::new(self.secret, self.nonce, server.nonce, Role::Client),
            reply,
        ))
    }
}

/// Server (`whisker run`) side of the handshake, between the server proof and the client proof
///
/// The dev server passes the first text message it receives to
/// [`respond`](Self::respond), sends the returned proof, and then sends
/// nothing else until [`finish`](Self::finish) accepts the client's proof.
/// The dev runtime relies on that silence to bound how much it reads before
/// authentication.
///
/// # Example
///
/// ```rust
/// use whisker_dev_protocol::{Error, ServerHandshake};
///
/// // A pre-handshake hello that carries the secret is not a challenge.
/// let legacy = r#"{"kind":"hello","aslr_reference":1,"token":"secret"}"#;
/// assert!(matches!(
///     ServerHandshake::respond("secret", legacy),
///     Err(Error::MalformedHandshake(_))
/// ));
/// ```
pub struct ServerHandshake {
    secret: String,
    client_nonce: [u8; 32],
    server_nonce: [u8; 32],
}

impl ServerHandshake {
    /// Check a client challenge and return the server proof text to send
    ///
    /// Fails with [`Error::UnsupportedVersion`] for a challenge from another
    /// protocol version and [`Error::MalformedHandshake`] for anything that
    /// is not a challenge.
    pub fn respond(secret: &str, challenge: &str) -> Result<(Self, String), Error> {
        if secret.is_empty() {
            return Err(Error::MissingSecret);
        }
        #[derive(Deserialize)]
        struct Version {
            version: u8,
        }
        if let Ok(Version { version }) = parse(challenge) {
            if version != PROTOCOL_VERSION {
                return Err(Error::UnsupportedVersion(version));
            }
        }
        let challenge: Challenge = parse(challenge)?;
        let server_nonce = random_bytes()?;
        let reply = encode(&ServerProof {
            nonce: server_nonce,
            mac: proof(secret, Role::Server, &challenge.nonce, &server_nonce),
        })?;
        Ok((
            Self {
                secret: secret.to_owned(),
                client_nonce: challenge.nonce,
                server_nonce,
            },
            reply,
        ))
    }

    /// Verify the client's proof and return the established session
    ///
    /// Fails with [`Error::InvalidClientProof`] when the client does not
    /// hold the same secret.
    pub fn finish(self, client_proof: &str) -> Result<Session, Error> {
        let client: ClientProof = parse(client_proof)?;
        if !verify_proof(
            &self.secret,
            Role::Client,
            &self.client_nonce,
            &self.server_nonce,
            &client.mac,
        ) {
            return Err(Error::InvalidClientProof);
        }
        Ok(Session::new(
            self.secret,
            self.client_nonce,
            self.server_nonce,
            Role::Server,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrameKind;

    #[test]
    fn matching_secrets_establish_paired_sessions() {
        let (client, challenge) = ClientHandshake::start("secret").unwrap();
        assert!(!challenge.contains("secret"));
        let (server, server_proof) = ServerHandshake::respond("secret", &challenge).unwrap();
        let (mut app, client_proof) = client.finish(&server_proof).unwrap();
        let mut host = server.finish(&client_proof).unwrap();
        let frame = host.seal(FrameKind::Patch, b"library").unwrap();
        assert_eq!(
            app.open(&frame).unwrap(),
            (FrameKind::Patch, b"library".as_slice())
        );
        let frame = app.seal(FrameKind::Text, b"hello").unwrap();
        assert_eq!(
            host.open(&frame).unwrap(),
            (FrameKind::Text, b"hello".as_slice())
        );
    }

    #[test]
    fn mismatched_secrets_fail_on_both_sides() {
        let (client, challenge) = ClientHandshake::start("app").unwrap();
        let (_, server_proof) = ServerHandshake::respond("host", &challenge).unwrap();
        assert!(matches!(
            client.finish(&server_proof),
            Err(Error::InvalidServerProof)
        ));

        let (client, challenge) = ClientHandshake::start("secret").unwrap();
        let (server, server_proof) = ServerHandshake::respond("secret", &challenge).unwrap();
        let (_, client_proof) = client.finish(&server_proof).unwrap();
        let (other, _) = ServerHandshake::respond("secret", &challenge).unwrap();
        assert!(matches!(
            other.finish(&client_proof),
            Err(Error::InvalidClientProof)
        ));
        assert!(server.finish(&client_proof).is_ok());
    }

    #[test]
    fn invalid_handshake_messages_are_rejected() {
        assert!(matches!(
            ClientHandshake::start(""),
            Err(Error::MissingSecret)
        ));
        assert!(matches!(
            ServerHandshake::respond("", "{}"),
            Err(Error::MissingSecret)
        ));
        let future = format!(r#"{{"version":2,"nonce":{:?}}}"#, [0u8; 32]);
        assert!(matches!(
            ServerHandshake::respond("secret", &future),
            Err(Error::UnsupportedVersion(2))
        ));
        assert!(matches!(
            ServerHandshake::respond("secret", r#"{"kind":"hello","aslr_reference":1}"#),
            Err(Error::MalformedHandshake(_))
        ));
        let padded = format!("{}{}", " ".repeat(MAX_HANDSHAKE), "{}");
        assert!(matches!(
            ServerHandshake::respond("secret", &padded),
            Err(Error::HandshakeTooLarge)
        ));
        let (client, _) = ClientHandshake::start("secret").unwrap();
        assert!(matches!(
            client.finish(&padded),
            Err(Error::HandshakeTooLarge)
        ));
    }
}
