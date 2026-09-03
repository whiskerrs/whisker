use super::*;

#[test]
fn generate_context_round_trips_through_json() {
    let mut ctx = GenerateContext {
        app_meta: AppMeta {
            name: "Demo".into(),
            version: "1.0".into(),
            build_number: 7,
            ios_bundle_id: Some("rs.whisker.demo".into()),
            android_application_id: Some("rs.whisker.demo".into()),
        },
        ios: Some(IosProjectIr::default()),
        android: Some(AndroidProjectIr::default()),
        journal: MutationJournal::default(),
        app_crate_dir: None,
    };
    ctx.ios.as_mut().unwrap().info_plist.insert(
        "CFBundleIdentifier".into(),
        PlistValue::String("rs.whisker.demo".into()),
    );
    ctx.android
        .as_mut()
        .unwrap()
        .manifest
        .permissions
        .push("android.permission.CAMERA".into());
    ctx.android
        .as_mut()
        .unwrap()
        .manifest
        .application_attributes
        .push(ApplicationAttribute {
            name: "android:enableOnBackInvokedCallback".into(),
            value: "true".into(),
        });
    ctx.journal.record(
        "whisker-info-plist",
        Target::Ios,
        "info_plist.CFBundleIdentifier",
        Operation::Set,
    );
    ctx.journal.record(
        "whisker-permissions",
        Target::Android,
        "manifest.permissions",
        Operation::ArrayPush { count: 1 },
    );

    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: GenerateContext = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.app_meta.name, "Demo");
    assert_eq!(back.journal.records.len(), 2);
    assert_eq!(back.journal.next_sequence_index, 2);
    assert_eq!(
        back.ios.unwrap().info_plist.get("CFBundleIdentifier"),
        Some(&PlistValue::String("rs.whisker.demo".into())),
    );
    let back_android = back.android.unwrap();
    assert_eq!(
        back_android.manifest.permissions,
        vec!["android.permission.CAMERA".to_string()],
    );
    assert_eq!(
        back_android.manifest.application_attributes,
        vec![ApplicationAttribute {
            name: "android:enableOnBackInvokedCallback".into(),
            value: "true".into(),
        }],
    );
}

#[test]
fn base64_round_trips_arbitrary_bytes() {
    for input in [
        &b""[..],
        &b"f"[..],
        &b"fo"[..],
        &b"foo"[..],
        &b"foob"[..],
        &b"fooba"[..],
        &b"foobar"[..],
        &[0u8, 1, 2, 253, 254, 255][..],
    ] {
        let encoded = base64_encode(input);
        assert!(encoded.is_ascii(), "base64 must be ASCII: {encoded}");
        let decoded = base64_decode(&encoded).expect("decode");
        assert_eq!(decoded, input, "round trip for {input:?}");
    }
}

#[test]
fn base64_matches_known_vectors() {
    // RFC 4648 test vectors.
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");
    assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
}

#[test]
fn base64_decode_rejects_garbage() {
    assert!(base64_decode("not valid!").is_err());
}

#[test]
fn file_entry_binary_round_trips_through_json() {
    let raw = &[0x89u8, 0x50, 0x4e, 0x47, 0x00, 0xff];
    let entry = FileEntry::binary(raw);
    let json = serde_json::to_string(&entry).unwrap();
    let back: FileEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.to_bytes().unwrap(), raw);
}

#[test]
fn file_entry_text_to_bytes_is_utf8() {
    let entry = FileEntry::text("hello");
    assert_eq!(entry.to_bytes().unwrap(), b"hello");
    assert!(entry.contents_base64.is_none());
}

#[test]
fn file_entry_text_default_decodes_without_base64_field() {
    let json = r#"{"contents":"old text"}"#;
    let entry: FileEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.to_bytes().unwrap(), b"old text");
}

#[test]
fn sequence_indices_are_monotonic() {
    let mut j = MutationJournal::default();
    j.record("a", Target::Ios, "x", Operation::Set);
    j.record("b", Target::Android, "y", Operation::Set);
    j.record("a", Target::Ios, "z", Operation::ArrayPush { count: 3 });
    let seqs: Vec<_> = j.records.iter().map(|r| r.sequence_index).collect();
    assert_eq!(seqs, vec![0, 1, 2]);
    assert_eq!(j.next_sequence_index, 3);
}

#[test]
fn pbxproj_ops_round_trip() {
    let ops = vec![
        PbxprojOp::AddResource {
            path: "GoogleService-Info.plist".into(),
        },
        PbxprojOp::LinkSystemFramework {
            name: "AVFoundation.framework".into(),
        },
        PbxprojOp::SetBuildSetting {
            key: "SWIFT_VERSION".into(),
            value: "5".into(),
        },
    ];
    let json = serde_json::to_string(&ops).unwrap();
    let back: Vec<PbxprojOp> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ops);
}

#[test]
fn plugin_request_envelope_round_trips() {
    let req = PluginRequest {
        name: "whisker-firebase".into(),
        config: serde_json::json!({"googleServicePath": "ios/GoogleService.plist"}),
        context: GenerateContext::default(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: PluginRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "whisker-firebase");
    assert_eq!(back.config["googleServicePath"], "ios/GoogleService.plist");
}

struct Null;

#[derive(Default, Serialize, Deserialize)]
struct NullConfig {
    #[allow(dead_code)]
    flag: bool,
}

impl PluginConfig for NullConfig {
    const NAME: &'static str = "null";
}

impl Plugin for Null {
    type Config = NullConfig;
    fn apply(&self, _ctx: &mut GenerateContext, _config: &Self::Config) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn plugin_trait_default_methods_work() {
    let p = Null;
    assert_eq!(p.name(), "null");
    assert!(p.after().is_empty());
    assert!(p.before().is_empty());
    let cfg = NullConfig::default();
    p.validate(&cfg).unwrap();
    let mut ctx = GenerateContext::default();
    p.apply(&mut ctx, &cfg).unwrap();
}

// In-memory stand-in for `run_as_subprocess`, whose stdin/stdout
// plumbing can't be driven from a unit test. Keep the two in sync.
fn run_with_pipes<P: Plugin>(plugin: P, input: &str) -> anyhow::Result<String> {
    let request: PluginRequest = serde_json::from_str(input)?;
    anyhow::ensure!(
        request.name == plugin.name(),
        "name mismatch: {} vs {}",
        request.name,
        plugin.name(),
    );
    let config: P::Config = serde_json::from_value(request.config)?;
    plugin.validate(&config)?;
    let mut ctx = request.context;
    plugin.apply(&mut ctx, &config)?;
    Ok(serde_json::to_string(&PluginResponse { context: ctx })?)
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PermissionConfig {
    permission: String,
}

impl PluginConfig for PermissionConfig {
    const NAME: &'static str = "test-permission";
}

struct Permission;

impl Plugin for Permission {
    type Config = PermissionConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &PermissionConfig) -> anyhow::Result<()> {
        let android = ctx
            .android
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("test-permission requires android target enabled"))?;
        android.manifest.permissions.push(cfg.permission.clone());
        ctx.journal.record(
            PermissionConfig::NAME,
            Target::Android,
            "manifest.permissions",
            Operation::ArrayPush { count: 1 },
        );
        Ok(())
    }
}

#[test]
fn subprocess_happy_path_round_trip() {
    let request = PluginRequest {
        name: "test-permission".into(),
        config: serde_json::json!({"permission": "android.permission.CAMERA"}),
        context: GenerateContext {
            android: Some(AndroidProjectIr::default()),
            ..Default::default()
        },
    };
    let input = serde_json::to_string(&request).unwrap();

    let output = run_with_pipes(Permission, &input).unwrap();
    let response: PluginResponse = serde_json::from_str(&output).unwrap();

    let android = response.context.android.expect("android should be present");
    assert_eq!(
        android.manifest.permissions,
        vec!["android.permission.CAMERA".to_string()],
    );
    assert_eq!(response.context.journal.records.len(), 1);
    assert_eq!(
        response.context.journal.records[0].plugin,
        "test-permission",
    );
    assert!(matches!(
        response.context.journal.records[0].operation,
        Operation::ArrayPush { count: 1 },
    ));
}

#[test]
fn subprocess_name_mismatch_is_an_error() {
    let request = PluginRequest {
        name: "some-other-plugin".into(),
        config: serde_json::json!({"permission": "x"}),
        context: GenerateContext::default(),
    };
    let input = serde_json::to_string(&request).unwrap();
    let err = run_with_pipes(Permission, &input).unwrap_err();
    assert!(err.to_string().contains("name mismatch"), "{err}");
}

#[test]
fn subprocess_apply_error_propagates() {
    let request = PluginRequest {
        name: "test-permission".into(),
        config: serde_json::json!({"permission": "android.permission.CAMERA"}),
        // Default context has no android IR, which `apply` requires.
        context: GenerateContext::default(),
    };
    let input = serde_json::to_string(&request).unwrap();
    let err = run_with_pipes(Permission, &input).unwrap_err();
    assert!(err.to_string().contains("requires android"), "{err}");
}
