//! Minimal App Store Connect API client — exactly enough for the
//! `whisker credential ios` wizard to validate a freshly created key
//! and resolve its team id. Deliberately not a general client:
//! builds authenticate through xcodebuild's own `-authenticationKey*`
//! flags, so the only REST consumer today is the wizard.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct KeyAuth<'a> {
    pub p8_pem: &'a str,
    pub key_id: &'a str,
    pub issuer_id: &'a str,
}

/// Build the ES256 bearer token (JWT) for one request burst.
///
/// Hand-rolled on purpose: the JWS is just
/// `b64url(header).b64url(payload)` signed with the .p8's P-256 key,
/// and doing it directly keeps us off ring/openssl-backed JWT crates.
fn bearer_token(auth: &KeyAuth) -> Result<String> {
    use p256::ecdsa::signature::Signer;
    use p256::pkcs8::DecodePrivateKey;

    let header = serde_json::json!({ "alg": "ES256", "kid": auth.key_id, "typ": "JWT" });
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before 1970")?
        .as_secs();
    // 10-minute expiry; Apple rejects tokens valid longer than 20.
    let payload = serde_json::json!({
        "iss": auth.issuer_id,
        "iat": now,
        "exp": now + 600,
        "aud": "appstoreconnect-v1",
    });
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?),
    );
    let key = p256::ecdsa::SigningKey::from_pkcs8_pem(auth.p8_pem)
        .map_err(|e| anyhow!("the .p8 file is not a valid PKCS#8 P-256 private key: {e}"))?;
    let signature: p256::ecdsa::Signature = key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// One authenticated GET against the ASC API, with the wizard's
/// error translation.
fn get(auth: &KeyAuth, path: &str) -> Result<serde_json::Value> {
    let token = bearer_token(auth)?;
    let url = format!("https://api.appstoreconnect.apple.com{path}");
    match ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(resp) => resp.into_json().context("parse ASC API response JSON"),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(translate_api_error(code, &body))
        }
        Err(e) => Err(e).with_context(|| format!("GET {url}")),
    }
}

/// Turn an ASC error response into an actionable message. The body's
/// `errors[0].code` is the real diagnosis — an Admin key can still
/// 403 for reasons that have nothing to do with roles (expired
/// license agreement, lapsed membership), so a canned role hint
/// alone MISDIAGNOSES those. Always surface Apple's own code +
/// detail, then add a translation for the cases we know.
fn translate_api_error(http_status: u16, body: &str) -> anyhow::Error {
    let first_error = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.pointer("/errors/0").cloned());
    let api_code = first_error
        .as_ref()
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let detail = first_error
        .as_ref()
        .and_then(|e| e.get("detail"))
        .and_then(|d| d.as_str())
        .unwrap_or("(no detail)")
        .to_string();

    let hint = if api_code.contains("REQUIRED_AGREEMENTS_MISSING_OR_EXPIRED") {
        "\nFix: the Apple Developer Program License Agreement needs (re-)acceptance.\n\
         Have the ACCOUNT HOLDER sign in to https://appstoreconnect.apple.com and\n\
         accept the pending agreement (the banner on the home page, or Business),\n\
         then re-run this command — the same key will work."
    } else if http_status == 401 {
        "\nFix: Key ID / Issuer ID mismatch, or the key was revoked. Individual keys\n\
         have no Issuer ID — make sure you created a TEAM key\n\
         (Users and Access → Integrations → Team Keys)."
    } else if http_status == 403 {
        "\nFix: the key lacks permission. Cloud-managed signing needs an Admin-role\n\
         TEAM key — create one under Team Keys with access = Admin."
    } else {
        ""
    };
    anyhow!("App Store Connect API error {http_status} ({api_code}): {detail}{hint}")
}

/// Cheapest authenticated call — proves key id + issuer id + .p8 are
/// a working combination.
pub fn validate(auth: &KeyAuth) -> Result<()> {
    get(auth, "/v1/apps?limit=1").map(|_| ())
}

/// The team id ("seed id") isn't a first-class API resource, but
/// every registered bundle id carries it as `seedId`. Returns
/// `None` for a brand-new team with no bundle ids yet — the wizard
/// falls back to asking.
pub fn resolve_team_id(auth: &KeyAuth) -> Result<Option<String>> {
    let json = get(auth, "/v1/bundleIds?limit=1")?;
    Ok(json
        .pointer("/data/0/attributes/seedId")
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::EncodePrivateKey;

    #[test]
    fn bearer_token_is_a_verifiable_es256_jws() {
        let signing_key = p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
        let pem = signing_key
            .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
            .unwrap();
        let auth = KeyAuth {
            p8_pem: &pem,
            key_id: "ABC123XYZ",
            issuer_id: "57246542-96fe-1a63-e053-0824d011072a",
        };
        let token = bearer_token(&auth).expect("token");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWS must be header.payload.signature");

        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], "ABC123XYZ");
        let payload: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(payload["aud"], "appstoreconnect-v1");
        assert_eq!(payload["iss"], auth.issuer_id);

        // The signature must be raw r||s over `header.payload`,
        // verifiable with the key's public half — exactly what
        // Apple's edge does.
        let sig_bytes = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let sig = p256::ecdsa::Signature::from_slice(&sig_bytes).expect("raw r||s signature");
        let verifying = p256::ecdsa::VerifyingKey::from(&signing_key);
        verifying
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .expect("signature verifies");
    }

    #[test]
    fn agreement_error_is_translated_not_misdiagnosed_as_role() {
        // Captured verbatim from a real Admin key blocked by an
        // unaccepted license agreement — the case a canned
        // "needs Admin role" hint gets wrong.
        let body = r#"{
  "errors" : [ {
    "id" : "43D2P7EFNE6CWFQL2BY3ZBPNQE",
    "status" : "403",
    "code" : "FORBIDDEN.REQUIRED_AGREEMENTS_MISSING_OR_EXPIRED",
    "title" : "A required agreement is missing or has expired.",
    "detail" : "This request requires an in-effect agreement that has not been signed or has expired.",
    "links" : { "see" : "/business" }
  } ]
}"#;
        let msg = translate_api_error(403, body).to_string();
        assert!(msg.contains("REQUIRED_AGREEMENTS_MISSING_OR_EXPIRED"));
        assert!(msg.contains("ACCOUNT HOLDER"));
        assert!(
            !msg.contains("Admin-role"),
            "agreement failures must not show the role hint: {msg}"
        );
    }

    #[test]
    fn plain_403_keeps_the_role_hint_and_unparseable_body_is_safe() {
        let msg = translate_api_error(403, "not json").to_string();
        assert!(msg.contains("Admin-role"));
        assert!(msg.contains("(no detail)"));
        let msg = translate_api_error(401, "{}").to_string();
        assert!(msg.contains("TEAM key"));
    }

    #[test]
    fn garbage_p8_is_a_clear_error() {
        let auth = KeyAuth {
            p8_pem: "not a pem",
            key_id: "X",
            issuer_id: "Y",
        };
        let err = bearer_token(&auth).unwrap_err();
        assert!(err.to_string().contains("PKCS#8"));
    }
}
