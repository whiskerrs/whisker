//! App Store Connect API client for the calls whisker makes itself:
//! key validation in the `whisker credential ios` wizard and build
//! upload in `whisker submit ios`. Builds authenticate through
//! xcodebuild's own `-authenticationKey*` flags instead.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
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

/// One authenticated ASC API call; a non-2xx answer comes back as
/// `Err((status, body))` for the caller to interpret. The token is
/// minted per call so a long upload between two calls can't outlive
/// its 10-minute expiry.
fn send(
    auth: &KeyAuth,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<std::result::Result<serde_json::Value, (u16, String)>> {
    let token = bearer_token(auth)?;
    let url = format!("https://api.appstoreconnect.apple.com{path}");
    let req = ureq::request(method, &url).set("Authorization", &format!("Bearer {token}"));
    let result = match body {
        Some(body) => req.send_json(body),
        None => req.call(),
    };
    match result {
        Ok(resp) => {
            let text = resp.into_string().context("read ASC API response")?;
            if text.trim().is_empty() {
                return Ok(Ok(serde_json::Value::Null));
            }
            serde_json::from_str(&text)
                .context("parse ASC API response JSON")
                .map(Ok)
        }
        Err(ureq::Error::Status(code, resp)) => {
            Ok(Err((code, resp.into_string().unwrap_or_default())))
        }
        Err(e) => Err(e).with_context(|| format!("{method} {url}")),
    }
}

pub(crate) fn request(
    auth: &KeyAuth,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    send(auth, method, path, body)?.map_err(|(code, body)| translate_api_error(code, &body))
}

fn get(auth: &KeyAuth, path: &str) -> Result<serde_json::Value> {
    request(auth, "GET", path, None)
}

/// GET where "no such resource" is an expected answer.
pub(crate) fn get_optional(auth: &KeyAuth, path: &str) -> Result<Option<serde_json::Value>> {
    match send(auth, "GET", path, None)? {
        Ok(json) => Ok(Some(json)),
        Err((404, _)) => Ok(None),
        Err((code, body)) => Err(translate_api_error(code, &body)),
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

/// App Store Connect's id for the app record with this bundle id.
/// `None` = no record yet; the API cannot create one.
pub fn find_app_id(auth: &KeyAuth, bundle_id: &str) -> Result<Option<String>> {
    let json = get(
        auth,
        &format!("/v1/apps?filter[bundleId]={bundle_id}&fields[apps]=bundleId"),
    )?;
    Ok(app_id_in(&json, bundle_id))
}

// `filter[bundleId]` can return other apps too, so match exactly.
pub(crate) fn app_id_in(json: &serde_json::Value, bundle_id: &str) -> Option<String> {
    json.get("data")?
        .as_array()?
        .iter()
        .find(|app| {
            app.pointer("/attributes/bundleId").and_then(|v| v.as_str()) == Some(bundle_id)
        })?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

/// One part of a reserved file upload: send `length` bytes starting
/// at `offset` to `url` with exactly these headers.
#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UploadOperation {
    pub method: String,
    pub url: String,
    pub offset: Option<u64>,
    pub length: Option<u64>,
    #[serde(default)]
    pub request_headers: Vec<HttpHeader>,
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

fn resource_id(json: &serde_json::Value, what: &str) -> Result<String> {
    json.pointer("/data/id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("App Store Connect returned no id for the new {what}"))
}

pub fn create_build_upload(
    auth: &KeyAuth,
    app_id: &str,
    short_version: &str,
    bundle_version: &str,
) -> Result<String> {
    let json = request(
        auth,
        "POST",
        "/v1/buildUploads",
        Some(serde_json::json!({
            "data": {
                "type": "buildUploads",
                "attributes": {
                    "cfBundleShortVersionString": short_version,
                    "cfBundleVersion": bundle_version,
                    "platform": "IOS",
                },
                "relationships": {
                    "app": { "data": { "type": "apps", "id": app_id } },
                },
            },
        })),
    )?;
    resource_id(&json, "build upload")
}

/// Declare the ipa and receive where to send its bytes.
pub fn reserve_build_upload_file(
    auth: &KeyAuth,
    upload_id: &str,
    file_name: &str,
    file_size: u64,
) -> Result<(String, Vec<UploadOperation>)> {
    let json = request(
        auth,
        "POST",
        "/v1/buildUploadFiles",
        Some(serde_json::json!({
            "data": {
                "type": "buildUploadFiles",
                "attributes": {
                    "assetType": "ASSET",
                    "fileName": file_name,
                    "fileSize": file_size,
                    "uti": "com.apple.ipa",
                },
                "relationships": {
                    "buildUpload": { "data": { "type": "buildUploads", "id": upload_id } },
                },
            },
        })),
    )?;
    let operations = json
        .pointer("/data/attributes/uploadOperations")
        .cloned()
        .map(serde_json::from_value::<Vec<UploadOperation>>)
        .transpose()
        .context("parse uploadOperations")?
        .unwrap_or_default();
    if operations.is_empty() {
        return Err(anyhow!("App Store Connect returned no upload operations"));
    }
    Ok((resource_id(&json, "build upload file")?, operations))
}

pub fn commit_build_upload_file(auth: &KeyAuth, file_id: &str) -> Result<()> {
    request(
        auth,
        "PATCH",
        &format!("/v1/buildUploadFiles/{file_id}"),
        Some(serde_json::json!({
            "data": {
                "type": "buildUploadFiles",
                "id": file_id,
                "attributes": { "uploaded": true },
            },
        })),
    )
    .map(|_| ())
}

/// PUT each reserved byte range of `ipa` to its upload URL.
pub fn upload_parts(ipa: &Path, size: u64, operations: &[UploadOperation]) -> Result<()> {
    let mut file = std::fs::File::open(ipa).with_context(|| format!("open {}", ipa.display()))?;
    for (index, op) in operations.iter().enumerate() {
        let offset = op.offset.unwrap_or(0);
        let length = op.length.unwrap_or(size - offset);
        file.seek(SeekFrom::Start(offset))?;
        // An explicit Content-Length keeps ureq from switching to
        // chunked transfer encoding, which the storage backend rejects.
        let mut req = ureq::request(&op.method, &op.url).set("Content-Length", &length.to_string());
        for header in &op.request_headers {
            req = req.set(&header.name, &header.value);
        }
        match req.send((&mut file).take(length)) {
            Ok(_) => {}
            Err(ureq::Error::Status(code, resp)) => bail!(
                "uploading part {}/{} failed ({code}): {}",
                index + 1,
                operations.len(),
                resp.into_string().unwrap_or_default(),
            ),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("upload part {}/{}", index + 1, operations.len()));
            }
        }
    }
    Ok(())
}

/// Apple's processing verdict for one build upload.
#[derive(Debug, PartialEq)]
pub struct BuildUploadStatus {
    /// `AWAITING_UPLOAD` | `PROCESSING` | `FAILED` | `COMPLETE`.
    pub state: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn build_upload_status(auth: &KeyAuth, upload_id: &str) -> Result<BuildUploadStatus> {
    let json = get(auth, &format!("/v1/buildUploads/{upload_id}"))?;
    Ok(status_in(&json))
}

fn status_in(json: &serde_json::Value) -> BuildUploadStatus {
    let state = json.pointer("/data/attributes/state");
    let details = |key: &str| -> Vec<String> {
        state
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|d| {
                        let field = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("");
                        format!("{} {}", field("code"), field("description"))
                            .trim()
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    BuildUploadStatus {
        state: state
            .and_then(|s| s.get("state"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        errors: details("errors"),
        warnings: details("warnings"),
    }
}

/// The processed build for one uploaded binary, once App Store
/// Connect has turned the upload into a build record.
pub fn find_build_id(
    auth: &KeyAuth,
    app_id: &str,
    short_version: &str,
    bundle_version: &str,
) -> Result<Option<String>> {
    let json = get(
        auth,
        &format!(
            "/v1/builds?filter[app]={app_id}&filter[version]={bundle_version}\
             &filter[preReleaseVersion.version]={short_version}\
             &filter[preReleaseVersion.platform]=IOS&limit=1"
        ),
    )?;
    Ok(json
        .pointer("/data/0/id")
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

/// Set TestFlight's "What to Test" for one locale of a build,
/// updating the localization if the build already has it.
pub fn set_beta_whats_new(auth: &KeyAuth, build_id: &str, locale: &str, text: &str) -> Result<()> {
    let existing = get(
        auth,
        &format!("/v1/builds/{build_id}/betaBuildLocalizations?limit=200"),
    )?;
    let body = match localization_id_in(&existing, locale) {
        Some(id) => {
            return request(
                auth,
                "PATCH",
                &format!("/v1/betaBuildLocalizations/{id}"),
                Some(serde_json::json!({
                    "data": {
                        "type": "betaBuildLocalizations",
                        "id": id,
                        "attributes": { "whatsNew": text },
                    },
                })),
            )
            .map(|_| ());
        }
        None => serde_json::json!({
            "data": {
                "type": "betaBuildLocalizations",
                "attributes": { "locale": locale, "whatsNew": text },
                "relationships": {
                    "build": { "data": { "type": "builds", "id": build_id } },
                },
            },
        }),
    };
    request(auth, "POST", "/v1/betaBuildLocalizations", Some(body)).map(|_| ())
}

fn localization_id_in(json: &serde_json::Value, locale: &str) -> Option<String> {
    json.get("data")?
        .as_array()?
        .iter()
        .find(|l| l.pointer("/attributes/locale").and_then(|v| v.as_str()) == Some(locale))?
        .get("id")?
        .as_str()
        .map(str::to_string)
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

    #[test]
    fn app_lookup_requires_an_exact_bundle_id_match() {
        let json = serde_json::json!({ "data": [
            { "id": "111", "attributes": { "bundleId": "com.example.app.dev" } },
            { "id": "222", "attributes": { "bundleId": "com.example.app" } },
        ]});
        assert_eq!(app_id_in(&json, "com.example.app").as_deref(), Some("222"));
        assert_eq!(app_id_in(&json, "com.example"), None);
    }

    #[test]
    fn an_existing_localization_is_found_by_exact_locale() {
        let json = serde_json::json!({ "data": [
            { "id": "a", "attributes": { "locale": "en-US" } },
            { "id": "b", "attributes": { "locale": "ja" } },
        ]});
        assert_eq!(localization_id_in(&json, "ja").as_deref(), Some("b"));
        assert_eq!(localization_id_in(&json, "en"), None);
    }

    #[test]
    fn upload_operations_parse_from_apples_shape() {
        let ops: Vec<UploadOperation> = serde_json::from_value(serde_json::json!([{
            "method": "PUT",
            "url": "https://example.invalid/part1",
            "length": 10,
            "offset": 0,
            "partNumber": 1,
            "requestHeaders": [{ "name": "Content-Type", "value": "application/octet-stream" }],
        }]))
        .unwrap();
        assert_eq!(ops[0].length, Some(10));
        assert_eq!(ops[0].request_headers[0].name, "Content-Type");
    }

    #[test]
    fn status_collects_state_details() {
        let json = serde_json::json!({ "data": { "attributes": { "state": {
            "state": "FAILED",
            "errors": [{ "code": "90208", "description": "Invalid bundle." }],
            "warnings": [],
        }}}});
        assert_eq!(
            status_in(&json),
            BuildUploadStatus {
                state: "FAILED".into(),
                errors: vec!["90208 Invalid bundle.".into()],
                warnings: vec![],
            }
        );
    }

    #[test]
    fn parts_are_sent_as_the_exact_byte_ranges_apple_asked_for() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut length = None;
                let mut chunked = false;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = Some(v.trim().parse::<usize>().unwrap());
                    }
                    chunked |= lower.starts_with("transfer-encoding:");
                    if line == "\r\n" {
                        break;
                    }
                }
                assert!(!chunked, "parts must not use chunked encoding");
                let mut body = vec![0u8; length.expect("Content-Length")];
                reader.read_exact(&mut body).unwrap();
                bodies.push(body);
                reader
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
            bodies
        });

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"0123456789").unwrap();
        let op = |offset, length| UploadOperation {
            method: "PUT".into(),
            url: format!("http://{addr}/part"),
            offset: Some(offset),
            length: Some(length),
            request_headers: vec![],
        };
        upload_parts(file.path(), 10, &[op(0, 6), op(6, 4)]).unwrap();
        assert_eq!(
            server.join().unwrap(),
            vec![b"012345".to_vec(), b"6789".to_vec()]
        );
    }
}
