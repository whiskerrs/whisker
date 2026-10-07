//! Google Play Developer API client — the service-account token
//! exchange plus the `edits` calls `whisker submit android` and the
//! `whisker credential playstore` wizard make.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use whisker_credentials::PlaystoreServiceAccount;

const API: &str = "https://androidpublisher.googleapis.com";
const SCOPE: &str = "https://www.googleapis.com/auth/androidpublisher";

/// The RS256 assertion Google's token endpoint exchanges for an
/// access token.
fn assertion(account: &PlaystoreServiceAccount, now: u64) -> Result<String> {
    let der: Vec<u8> = STANDARD
        .decode(
            account
                .private_key
                .lines()
                .filter(|line| !line.starts_with("-----"))
                .collect::<String>(),
        )
        .map_err(|e| anyhow!("the service-account private_key is not valid PEM: {e}"))?;
    let key = ring::signature::RsaKeyPair::from_pkcs8(&der)
        .map_err(|e| anyhow!("the service-account private_key is not a PKCS#8 RSA key: {e}"))?;

    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
    let claims = serde_json::json!({
        "iss": account.client_email,
        "scope": SCOPE,
        "aud": account.token_uri,
        "iat": now,
        "exp": now + 3600,
    });
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?),
    );
    let mut signature = vec![0u8; key.public().modulus_len()];
    key.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_| anyhow!("signing the service-account assertion failed"))?;
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

/// An authenticated session for one package. The access token lasts
/// an hour — far longer than any single submit.
pub struct Client<'a> {
    token: String,
    account: &'a PlaystoreServiceAccount,
    package: &'a str,
}

enum Body<'a> {
    None,
    Json(serde_json::Value),
    File(&'a Path),
}

fn send(req: ureq::Request, body: Body) -> Result<Result<ureq::Response, ureq::Error>> {
    Ok(match body {
        // Google answers a bodyless POST that carries no Content-Length
        // with 411, so an explicit empty body is sent instead.
        Body::None => req.send_bytes(&[]),
        Body::Json(json) => req.send_json(json),
        Body::File(path) => {
            let file =
                std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
            let len = file.metadata()?.len();
            req.set("Content-Type", "application/octet-stream")
                .set("Content-Length", &len.to_string())
                .send(file)
        }
    })
}

impl<'a> Client<'a> {
    /// Exchange the service-account key for an access token. Proves
    /// the key itself works; says nothing about Play Console access.
    pub fn connect(account: &'a PlaystoreServiceAccount, package: &'a str) -> Result<Self> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock before 1970")?
            .as_secs();
        let jwt = assertion(account, now)?;
        let resp = ureq::post(&account.token_uri).send_form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ]);
        let json: serde_json::Value = match resp {
            Ok(resp) => resp.into_json().context("parse Google token response")?,
            Err(ureq::Error::Status(code, resp)) => bail!(
                "Google rejected the service-account key ({code}): {}\n\
                 Fix: the key was deleted or disabled — re-run `whisker credential playstore`.",
                resp.into_string().unwrap_or_default(),
            ),
            Err(e) => return Err(e).with_context(|| format!("POST {}", account.token_uri)),
        };
        let token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Google token response has no access_token"))?
            .to_string();
        Ok(Self {
            token,
            account,
            package,
        })
    }

    fn call(&self, method: &str, path: &str, body: Body) -> Result<serde_json::Value> {
        let url = format!("{API}{path}");
        let req =
            ureq::request(method, &url).set("Authorization", &format!("Bearer {}", self.token));
        let result = send(req, body)?;
        match result {
            Ok(resp) => {
                let text = resp.into_string().context("read Play API response")?;
                if text.trim().is_empty() {
                    return Ok(serde_json::Value::Null);
                }
                serde_json::from_str(&text).context("parse Play API response JSON")
            }
            Err(ureq::Error::Status(code, resp)) => Err(translate_api_error(
                code,
                &resp.into_string().unwrap_or_default(),
                &self.account.client_email,
                self.package,
            )),
            Err(e) => Err(e).with_context(|| format!("{method} {url}")),
        }
    }

    fn edits(&self) -> String {
        format!("/androidpublisher/v3/applications/{}/edits", self.package)
    }

    /// Open an edit — the transaction every Play change happens in.
    pub fn insert_edit(&self) -> Result<String> {
        let json = self.call("POST", &self.edits(), Body::None)?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Play API returned no edit id"))
    }

    pub fn delete_edit(&self, edit: &str) -> Result<()> {
        self.call("DELETE", &format!("{}/{edit}", self.edits()), Body::None)
            .map(|_| ())
    }

    /// Upload the bundle into `edit`; returns the versionCode Play
    /// read out of it.
    pub fn upload_bundle(&self, edit: &str, aab: &Path) -> Result<i64> {
        let json = self.call(
            "POST",
            &format!(
                "/upload/androidpublisher/v3/applications/{}/edits/{edit}/bundles?uploadType=media",
                self.package
            ),
            Body::File(aab),
        )?;
        json.get("versionCode")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow!("Play API returned no versionCode for the uploaded bundle"))
    }

    /// Put `version_code` on `track` as its single release.
    pub fn set_track_release(
        &self,
        edit: &str,
        track: &str,
        version_code: i64,
        status: &str,
    ) -> Result<()> {
        self.call(
            "PUT",
            &format!("{}/{edit}/tracks/{track}", self.edits()),
            Body::Json(serde_json::json!({
                "track": track,
                "releases": [{
                    "versionCodes": [version_code.to_string()],
                    "status": status,
                }],
            })),
        )
        .map(|_| ())
    }

    pub fn commit_edit(&self, edit: &str) -> Result<()> {
        self.call(
            "POST",
            &format!("{}/{edit}:commit", self.edits()),
            Body::None,
        )
        .map(|_| ())
    }
}

/// Turn a Play API error into an actionable message. Google's own
/// `error.message` is always shown; hints are added for the failures
/// every first submit runs into.
fn translate_api_error(
    http_status: u16,
    body: &str,
    client_email: &str,
    package: &str,
) -> anyhow::Error {
    let error = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").cloned());
    let message = error
        .as_ref()
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("(no detail)")
        .to_string();

    let hint = if message.contains("has not been used in project")
        || message.contains("is disabled")
    {
        "\nFix: enable the Google Play Android Developer API in the service account's project:\n\
         \x20 https://console.cloud.google.com/apis/library/androidpublisher.googleapis.com"
            .to_string()
    } else if message.contains("draft app") {
        "\nFix: this app has never been published, so Play only accepts draft releases —\n\
         re-run with `--draft` and roll the release out from Play Console."
            .to_string()
    } else if message.contains("already been used") {
        "\nFix: Play has seen this versionCode before — bump `build_number` in whisker.rs\n\
         and rebuild."
            .to_string()
    } else if http_status == 401 || http_status == 403 {
        format!(
            "\nFix: invite {client_email} in Play Console → Users and permissions, and give it\n\
             release permission for {package}. A fresh invite can take a few minutes to apply."
        )
    } else if http_status == 404 {
        format!(
            "\nFix: no app with applicationId {package} in this Play developer account. The API\n\
             cannot create apps — create it in Play Console and upload its first build there."
        )
    } else {
        String::new()
    };
    anyhow!("Google Play API error {http_status}: {message}{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ring can't generate RSA keys and a committed PEM trips secret
    // scanners, so the test key comes from the system openssl.
    fn test_key() -> Option<String> {
        let out = std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
            ])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap())
    }

    #[test]
    fn assertion_is_a_verifiable_rs256_jws() {
        let Some(test_key) = test_key() else {
            eprintln!("skipped: openssl not available");
            return;
        };
        let account = PlaystoreServiceAccount {
            client_email: "whisker@example.iam.gserviceaccount.com".into(),
            private_key: test_key.clone(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
        };
        let jwt = assertion(&account, 1_700_000_000).expect("assertion");
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["iss"], account.client_email);
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["aud"], account.token_uri);
        assert_eq!(claims["exp"], 1_700_003_600u64);

        let der = STANDARD
            .decode(
                test_key
                    .lines()
                    .filter(|l| !l.starts_with("-----"))
                    .collect::<String>(),
            )
            .unwrap();
        let key = ring::signature::RsaKeyPair::from_pkcs8(&der).unwrap();
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            key.public().as_ref(),
        )
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .expect("signature verifies");
    }

    #[test]
    fn a_bodyless_post_declares_an_empty_body() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                headers.push(line.trim().to_ascii_lowercase());
            }
            reader
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            headers
        });

        send(ureq::post(&format!("http://{addr}/edits")), Body::None)
            .unwrap()
            .unwrap();
        let headers = server.join().unwrap();
        assert!(
            headers.iter().any(|h| h == "content-length: 0"),
            "{headers:?}"
        );
    }

    #[test]
    fn garbage_private_key_is_a_clear_error() {
        let account = PlaystoreServiceAccount {
            client_email: "a@b".into(),
            private_key: "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n".into(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
        };
        let err = assertion(&account, 0).unwrap_err();
        assert!(err.to_string().contains("PKCS#8 RSA"));
    }

    #[test]
    fn errors_keep_googles_message_and_add_the_matching_hint() {
        let body =
            |message: &str| serde_json::json!({ "error": { "message": message } }).to_string();
        let translate = |status, message: &str| {
            translate_api_error(status, &body(message), "sa@p.iam", "com.example.app").to_string()
        };

        let msg = translate(403, "The caller does not have permission");
        assert!(msg.contains("The caller does not have permission"));
        assert!(msg.contains("invite sa@p.iam"));

        // A disabled API is also a 403; it must not get the invite hint.
        let msg = translate(
            403,
            "Google Play Android Developer API has not been used in project 1 before or it is disabled.",
        );
        assert!(msg.contains("apis/library/androidpublisher"));
        assert!(!msg.contains("invite"));

        assert!(
            translate(
                400,
                "Only releases with status draft may be created on draft app."
            )
            .contains("--draft")
        );
        assert!(translate(403, "Version code 3 has already been used.").contains("build_number"));
        assert!(
            translate(404, "Package not found: com.example.app.").contains("cannot create apps")
        );
        assert!(
            translate_api_error(500, "not json", "a", "b")
                .to_string()
                .contains("(no detail)")
        );
    }
}
