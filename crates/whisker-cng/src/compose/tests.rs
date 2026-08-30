use super::*;
use serde::{Deserialize, Serialize};
use whisker_plugin::{PlistValue, PluginConfig};

#[test]
fn every_app_gets_orientations_because_the_store_rejects_a_bundle_without_them() {
    let seeded = seed_orientation_plist(&[]);
    let PlistValue::Array(phone) = &seeded["UISupportedInterfaceOrientations"] else {
        panic!("expected an array");
    };
    assert_eq!(phone.len(), 4);
    assert_eq!(
        seeded["UISupportedInterfaceOrientations"],
        seeded["UISupportedInterfaceOrientations~ipad"]
    );
    assert!(!seeded.contains_key("UIRequiresFullScreen"));
}

#[test]
fn restricting_orientations_opts_out_of_ipad_multitasking() {
    let seeded = seed_orientation_plist(&[whisker_config::Orientation::Portrait]);
    assert_eq!(
        seeded["UISupportedInterfaceOrientations"],
        PlistValue::Array(vec![PlistValue::String(
            "UIInterfaceOrientationPortrait".to_string()
        )])
    );
    assert_eq!(
        seeded["UIRequiresFullScreen"],
        PlistValue::Boolean(true),
        "Apple only allows fewer than four orientations when the app opts out"
    );
}

#[derive(Default, Serialize, Deserialize)]
struct BundleIdConfig {
    #[serde(default)]
    suffix: String,
}
impl PluginConfig for BundleIdConfig {
    const NAME: &'static str = "set-bundle-id";
}
struct BundleId;
impl Plugin for BundleId {
    type Config = BundleIdConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &BundleIdConfig) -> Result<()> {
        let bundle_id = format!("{}{}", "rs.whisker.demo", cfg.suffix);
        if let Some(ios) = ctx.ios.as_mut() {
            ios.info_plist
                .insert("CFBundleIdentifier".into(), PlistValue::String(bundle_id));
            ctx.journal.record(
                Self::Config::NAME,
                Target::Ios,
                "info_plist.CFBundleIdentifier",
                Operation::Set,
            );
        }
        Ok(())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct PermissionsConfig {
    #[serde(default)]
    permissions: Vec<String>,
}
impl PluginConfig for PermissionsConfig {
    const NAME: &'static str = "permissions";
}
struct Permissions;
impl Plugin for Permissions {
    type Config = PermissionsConfig;
    fn apply(&self, ctx: &mut GenerateContext, cfg: &PermissionsConfig) -> Result<()> {
        if let Some(a) = ctx.android.as_mut() {
            for p in &cfg.permissions {
                a.manifest.permissions.push(p.clone());
            }
            if !cfg.permissions.is_empty() {
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush {
                        count: cfg.permissions.len(),
                    },
                );
            }
        }
        Ok(())
    }
}

/// Conflicts with BundleId if both `Set` the same key.
#[derive(Default, Serialize, Deserialize)]
struct AnotherBundleIdConfig {}
impl PluginConfig for AnotherBundleIdConfig {
    const NAME: &'static str = "another-bundle-id";
}
struct AnotherBundleId;
impl Plugin for AnotherBundleId {
    type Config = AnotherBundleIdConfig;
    fn apply(&self, ctx: &mut GenerateContext, _cfg: &AnotherBundleIdConfig) -> Result<()> {
        if let Some(ios) = ctx.ios.as_mut() {
            ios.info_plist.insert(
                "CFBundleIdentifier".into(),
                PlistValue::String("rs.other".into()),
            );
            ctx.journal.record(
                Self::Config::NAME,
                Target::Ios,
                "info_plist.CFBundleIdentifier",
                Operation::Set,
            );
        }
        Ok(())
    }
}

/// Like AnotherBundleId but uses Override.
#[derive(Default, Serialize, Deserialize)]
struct OverrideBundleIdConfig {}
impl PluginConfig for OverrideBundleIdConfig {
    const NAME: &'static str = "override-bundle-id";
}
struct OverrideBundleId;
impl Plugin for OverrideBundleId {
    type Config = OverrideBundleIdConfig;
    fn after(&self) -> &'static [&'static str] {
        &["set-bundle-id"]
    }
    fn apply(&self, ctx: &mut GenerateContext, _cfg: &OverrideBundleIdConfig) -> Result<()> {
        if let Some(ios) = ctx.ios.as_mut() {
            ios.info_plist.insert(
                "CFBundleIdentifier".into(),
                PlistValue::String("rs.overridden".into()),
            );
            ctx.journal.record(
                Self::Config::NAME,
                Target::Ios,
                "info_plist.CFBundleIdentifier",
                Operation::Override,
            );
        }
        Ok(())
    }
}

fn base_app_config() -> Config {
    let mut a = Config::default();
    a.name("Demo").bundle_id("rs.whisker.demo");
    a
}

#[test]
fn empty_engine_yields_an_empty_context() {
    let engine = Engine::new();
    let ctx = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap();
    assert!(ctx.ios.is_some());
    assert!(ctx.android.is_some());
    assert!(ctx.journal.records.is_empty());
}

#[test]
fn enabled_targets_control_which_ir_is_populated() {
    let engine = Engine::new();
    let ios_only = engine
        .compose(&base_app_config(), EnabledTargets::ios_only())
        .unwrap();
    assert!(ios_only.ios.is_some());
    assert!(ios_only.android.is_none());
    let android_only = engine
        .compose(&base_app_config(), EnabledTargets::android_only())
        .unwrap();
    assert!(android_only.ios.is_none());
    assert!(android_only.android.is_some());
}

#[test]
fn appmeta_is_populated_from_app_config() {
    let engine = Engine::new();
    let ctx = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap();
    assert_eq!(ctx.app_meta.name, "Demo");
    assert_eq!(
        ctx.app_meta.ios_bundle_id.as_deref(),
        Some("rs.whisker.demo")
    );
    assert_eq!(
        ctx.app_meta.android_application_id.as_deref(),
        Some("rs.whisker.demo"),
    );
}

#[test]
fn plugin_runs_with_user_config_when_declared_in_app_config() {
    let mut engine = Engine::new();
    engine.register(BundleId);
    let mut app = base_app_config();
    app.plugin::<BundleId>(|c| {
        c.suffix = ".staging".into();
    });
    let ctx = engine.compose(&app, EnabledTargets::ios_only()).unwrap();
    let ios = ctx.ios.unwrap();
    assert_eq!(
        ios.info_plist.get("CFBundleIdentifier"),
        Some(&PlistValue::String("rs.whisker.demo.staging".into())),
    );
}

#[test]
fn plugin_falls_back_to_default_config_when_not_declared() {
    let mut engine = Engine::new();
    engine.register(BundleId);
    let ctx = engine
        .compose(&base_app_config(), EnabledTargets::ios_only())
        .unwrap();
    let ios = ctx.ios.unwrap();
    assert_eq!(
        ios.info_plist.get("CFBundleIdentifier"),
        Some(&PlistValue::String("rs.whisker.demo".into())),
    );
}

#[test]
fn after_constraint_orders_dependent_plugin_later() {
    let mut engine = Engine::new();
    engine.register(OverrideBundleId).register(BundleId);
    let ctx = engine
        .compose(&base_app_config(), EnabledTargets::ios_only())
        .unwrap();
    let ios = ctx.ios.unwrap();
    assert_eq!(
        ios.info_plist.get("CFBundleIdentifier"),
        Some(&PlistValue::String("rs.overridden".into())),
    );
    let seqs: Vec<_> = ctx
        .journal
        .records
        .iter()
        .map(|r| r.plugin.as_str())
        .collect();
    assert_eq!(seqs, vec!["set-bundle-id", "override-bundle-id"]);
}

#[test]
fn cycle_in_after_constraints_is_rejected() {
    struct A;
    struct B;
    #[derive(Default, Serialize, Deserialize)]
    struct ConfigA;
    impl PluginConfig for ConfigA {
        const NAME: &'static str = "a";
    }
    #[derive(Default, Serialize, Deserialize)]
    struct ConfigB;
    impl PluginConfig for ConfigB {
        const NAME: &'static str = "b";
    }
    impl Plugin for A {
        type Config = ConfigA;
        fn after(&self) -> &'static [&'static str] {
            &["b"]
        }
        fn apply(&self, _: &mut GenerateContext, _: &ConfigA) -> Result<()> {
            Ok(())
        }
    }
    impl Plugin for B {
        type Config = ConfigB;
        fn after(&self) -> &'static [&'static str] {
            &["a"]
        }
        fn apply(&self, _: &mut GenerateContext, _: &ConfigB) -> Result<()> {
            Ok(())
        }
    }
    let mut engine = Engine::new();
    engine.register(A).register(B);
    let err = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("cycle"), "{msg}");
}

#[test]
fn after_referencing_an_unregistered_plugin_is_rejected() {
    struct A;
    #[derive(Default, Serialize, Deserialize)]
    struct ConfigA;
    impl PluginConfig for ConfigA {
        const NAME: &'static str = "a";
    }
    impl Plugin for A {
        type Config = ConfigA;
        fn after(&self) -> &'static [&'static str] {
            &["non-existent"]
        }
        fn apply(&self, _: &mut GenerateContext, _: &ConfigA) -> Result<()> {
            Ok(())
        }
    }
    let mut engine = Engine::new();
    engine.register(A);
    let err = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("non-existent"), "{msg}");
}

#[test]
fn declaring_an_unknown_plugin_in_app_config_is_rejected() {
    let mut app = base_app_config();
    app.plugins
        .insert("ghost-plugin".to_string(), serde_json::json!({}));
    let engine = Engine::new();
    let err = engine.compose(&app, EnabledTargets::both()).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("ghost-plugin"), "{msg}");
}

#[test]
fn duplicate_plugin_registration_is_rejected() {
    let mut engine = Engine::new();
    engine.register(BundleId).register(BundleId);
    let err = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("set-bundle-id"), "{msg}");
}

#[test]
fn validate_failure_aborts_before_apply_runs() {
    struct Picky;
    #[derive(Default, Serialize, Deserialize)]
    struct PickyConfig;
    impl PluginConfig for PickyConfig {
        const NAME: &'static str = "picky";
    }
    impl Plugin for Picky {
        type Config = PickyConfig;
        fn validate(&self, _: &PickyConfig) -> Result<()> {
            bail!("nope")
        }
        fn apply(&self, _: &mut GenerateContext, _: &PickyConfig) -> Result<()> {
            panic!("apply should not run when validate fails")
        }
    }
    let mut engine = Engine::new();
    engine.register(Picky);
    let err = engine
        .compose(&base_app_config(), EnabledTargets::both())
        .unwrap_err();
    assert!(format!("{err:#}").contains("nope"));
}

#[test]
fn two_set_writes_to_same_path_is_a_conflict() {
    let mut engine = Engine::new();
    engine.register(BundleId).register(AnotherBundleId);
    let err = engine
        .compose(&base_app_config(), EnabledTargets::ios_only())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("CFBundleIdentifier"), "{msg}");
    assert!(msg.contains("set-bundle-id"), "{msg}");
    assert!(msg.contains("another-bundle-id"), "{msg}");
}

#[test]
fn override_resolves_what_would_otherwise_be_a_conflict() {
    let mut engine = Engine::new();
    engine.register(BundleId).register(OverrideBundleId);
    engine
        .compose(&base_app_config(), EnabledTargets::ios_only())
        .expect("override should resolve the would-be conflict");
}

#[test]
fn array_push_never_conflicts_even_across_plugins() {
    struct OneCam;
    struct OneLoc;
    #[derive(Default, Serialize, Deserialize)]
    struct C1;
    impl PluginConfig for C1 {
        const NAME: &'static str = "one-cam";
    }
    #[derive(Default, Serialize, Deserialize)]
    struct C2;
    impl PluginConfig for C2 {
        const NAME: &'static str = "one-loc";
    }
    impl Plugin for OneCam {
        type Config = C1;
        fn apply(&self, ctx: &mut GenerateContext, _: &C1) -> Result<()> {
            if let Some(a) = ctx.android.as_mut() {
                a.manifest
                    .permissions
                    .push("android.permission.CAMERA".into());
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush { count: 1 },
                );
            }
            Ok(())
        }
    }
    impl Plugin for OneLoc {
        type Config = C2;
        fn apply(&self, ctx: &mut GenerateContext, _: &C2) -> Result<()> {
            if let Some(a) = ctx.android.as_mut() {
                a.manifest
                    .permissions
                    .push("android.permission.LOCATION".into());
                ctx.journal.record(
                    Self::Config::NAME,
                    Target::Android,
                    "manifest.permissions",
                    Operation::ArrayPush { count: 1 },
                );
            }
            Ok(())
        }
    }
    let mut engine = Engine::new();
    engine.register(OneCam).register(OneLoc);
    let ctx = engine
        .compose(&base_app_config(), EnabledTargets::android_only())
        .unwrap();
    let perms = ctx.android.unwrap().manifest.permissions;
    assert_eq!(perms.len(), 2);
}

#[test]
fn config_decode_error_is_surfaced_with_plugin_name() {
    let mut app = base_app_config();
    app.plugins.insert(
        BundleIdConfig::NAME.to_string(),
        serde_json::json!({"suffix": 7}),
    );
    let mut engine = Engine::new();
    engine.register(BundleId);
    let err = engine
        .compose(&app, EnabledTargets::ios_only())
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("set-bundle-id"), "{msg}");
    assert!(msg.contains("decode"), "{msg}");
}

#[test]
fn full_pipeline_with_permissions_and_bundle_id_succeeds() {
    let mut app = base_app_config();
    app.plugin::<BundleId>(|c| {
        c.suffix = ".dev".into();
    });
    app.plugin::<Permissions>(|c| {
        c.permissions.extend([
            "android.permission.CAMERA".into(),
            "android.permission.LOCATION".into(),
        ]);
    });

    let mut engine = Engine::new();
    engine.register(BundleId).register(Permissions);
    let ctx = engine.compose(&app, EnabledTargets::both()).unwrap();

    assert_eq!(
        ctx.ios.as_ref().unwrap().info_plist["CFBundleIdentifier"],
        PlistValue::String("rs.whisker.demo.dev".into()),
    );
    assert_eq!(ctx.android.as_ref().unwrap().manifest.permissions.len(), 2);
    assert_eq!(ctx.journal.records.len(), 2);
}

#[test]
fn build_request_carries_name_config_and_full_context() {
    let mut ctx = GenerateContext::default();
    ctx.app_meta.name = "Demo".into();
    ctx.journal.record(
        "earlier-plugin",
        Target::Ios,
        "info_plist.X",
        Operation::Set,
    );
    let req = build_request(
        "my-plugin".into(),
        Some(&serde_json::json!({"opt": true})),
        &ctx,
    );
    assert_eq!(req.name, "my-plugin");
    assert_eq!(req.config["opt"], true);
    assert_eq!(req.context.journal.next_sequence_index, 1);
    assert_eq!(req.context.app_meta.name, "Demo");
}

#[test]
fn build_request_uses_null_for_missing_user_config() {
    let ctx = GenerateContext::default();
    let req = build_request("my-plugin".into(), None, &ctx);
    assert!(req.config.is_null());
}

#[test]
fn merge_response_replaces_the_engine_context() {
    let mut ctx = GenerateContext::default();
    ctx.app_meta.name = "Old".into();
    let mut new_ctx = GenerateContext::default();
    new_ctx.app_meta.name = "New".into();
    new_ctx.journal.record(
        "subprocess-plugin",
        Target::Android,
        "manifest.permissions",
        Operation::ArrayPush { count: 1 },
    );
    merge_response(&mut ctx, PluginResponse { context: new_ctx });
    assert_eq!(ctx.app_meta.name, "New");
    assert_eq!(ctx.journal.records.len(), 1);
}

#[test]
fn decode_response_bytes_handles_empty_stdout() {
    let err = decode_response_bytes("p", b"").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("empty"), "{msg}");
    assert!(msg.contains("`p`"), "{msg}");
}

#[test]
fn decode_response_bytes_surfaces_invalid_json_with_plugin_name() {
    let err = decode_response_bytes("p", b"not json").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("`p`"), "{msg}");
    assert!(msg.contains("decode"), "{msg}");
}

#[test]
fn decode_response_bytes_accepts_a_valid_envelope() {
    let envelope = serde_json::to_vec(&PluginResponse {
        context: GenerateContext::default(),
    })
    .unwrap();
    let resp = decode_response_bytes("p", &envelope).unwrap();
    assert_eq!(resp.context.journal.records.len(), 0);
}
