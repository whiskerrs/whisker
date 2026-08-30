use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use cbindgen::{Config, Language, RenameRule};

const HEADER_PATHS: [&str; 2] = [
    "crates/whisker-driver-sys/bridge/include/whisker_mobile.h",
    "platforms/ios/Sources/WhiskerCBridge/include/whisker_mobile.h",
];
const KOTLIN_PATH: &str =
    "platforms/android/runtime/src/main/kotlin/rs/whisker/runtime/bridge/MobileAbi.kt";
const ANDROID_JNI_HEADER_PATH: &str =
    "crates/whisker-driver-sys/bridge/include/whisker_android_jni.h";
const ANDROID_JNI_KOTLIN_PATH: &str =
    "platforms/android/runtime/src/main/kotlin/rs/whisker/runtime/bridge/AndroidFrameBatch.kt";

const ANDROID_OPERATION_FIELDS: &[&str] = &[
    "TAG", "FLAGS", "NODE", "PARENT", "CHILD", "INDEX", "MEMBER", "INTEGER", "SCALAR", "WIDE",
];

struct GeneratedAbi {
    header: Vec<u8>,
    kotlin: Vec<u8>,
    android_jni_header: Vec<u8>,
    android_jni_kotlin: Vec<u8>,
}

const MOBILE_TYPES: &[&str] = &[
    "MobileRect",
    "MobileLayoutGeometry",
    "MobileColor",
    "MobileLengthPercentage",
    "MobileBoxPaint",
    "MobileBoxShadow",
    "MobileClipInset",
    "MobileClipCircle",
    "MobileClipEllipse",
    "MobilePathCommand",
    "MobileClipPathCommands",
    "MobileClipPath",
    "MobileGradientStop",
    "MobileRadialGradient",
    "MobileConicGradient",
    "MobileBackgroundImage",
    "MobileBackgroundLayer",
    "MobileFontFeature",
    "MobileFontVariation",
    "MobileText",
    "MobileOperation",
    "MobileFrame",
    "MobileApplyResponse",
    "MobileMemberRegistration",
    "MobileElementRegistration",
    "MobileBootstrap",
    "MobileMeasureRequest",
    "MobileMeasureResponse",
    "MobileResourceCommand",
    "MobileResourceEvent",
];

const CALLBACKS: &[(&str, &str)] = &[
    ("RequestFrameCallback", "WhiskerMobileRequestFrameCallback"),
    ("BootstrapCallback", "WhiskerMobileBootstrapCallback"),
    ("MeasureCallback", "WhiskerMobileMeasureCallback"),
    ("PresentFrameCallback", "WhiskerMobilePresentFrameCallback"),
    (
        "ResourceCommandCallback",
        "WhiskerMobileResourceCommandCallback",
    ),
    ("ModuleResultCallback", "WhiskerMobileModuleResultCallback"),
    ("InvokeModuleCallback", "WhiskerMobileInvokeModuleCallback"),
    (
        "ObserveModuleCallback",
        "WhiskerMobileObserveModuleCallback",
    ),
];

pub fn run(root: &Path, mode: &str) -> Result<()> {
    let generated = generate(root)?;
    match mode {
        "generate" => {
            for relative in HEADER_PATHS {
                let path = root.join(relative);
                fs::write(&path, &generated.header)
                    .with_context(|| format!("write {}", path.display()))?;
            }
            let kotlin = root.join(KOTLIN_PATH);
            if let Some(parent) = kotlin.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&kotlin, &generated.kotlin)
                .with_context(|| format!("write {}", kotlin.display()))?;
            write_generated(root, ANDROID_JNI_HEADER_PATH, &generated.android_jni_header)?;
            write_generated(root, ANDROID_JNI_KOTLIN_PATH, &generated.android_jni_kotlin)?;
            Ok(())
        }
        "check" => {
            for relative in HEADER_PATHS {
                let path = root.join(relative);
                let checked_in =
                    fs::read(&path).with_context(|| format!("read {}", path.display()))?;
                if checked_in != generated.header {
                    bail!(
                        "{} is stale; run `cargo xtask mobile-abi generate`",
                        path.display()
                    );
                }
            }
            let kotlin = root.join(KOTLIN_PATH);
            let checked_in =
                fs::read(&kotlin).with_context(|| format!("read {}", kotlin.display()))?;
            if checked_in != generated.kotlin {
                bail!(
                    "{} is stale; run `cargo xtask mobile-abi generate`",
                    kotlin.display()
                );
            }
            check_generated(root, ANDROID_JNI_HEADER_PATH, &generated.android_jni_header)?;
            check_generated(root, ANDROID_JNI_KOTLIN_PATH, &generated.android_jni_kotlin)?;
            Ok(())
        }
        _ => bail!("unknown mobile-abi mode {mode:?}; expected generate or check"),
    }
}

fn generate(root: &Path) -> Result<GeneratedAbi> {
    let crate_root = root.join("crates/whisker-driver-sys");
    let source = ["src/lib.rs", "src/mobile.rs"]
        .into_iter()
        .map(|relative| {
            fs::read_to_string(crate_root.join(relative))
                .with_context(|| format!("read mobile ABI source {relative}"))
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n");

    let mut config = Config {
        language: Language::C,
        include_guard: Some("WHISKER_MOBILE_H_".into()),
        cpp_compat: true,
        usize_is_size_t: true,
        documentation: true,
        autogen_warning: Some(
            "/* Generated by `cargo xtask mobile-abi generate`; do not edit. */".into(),
        ),
        trailer: Some(layout_assertions().into()),
        ..Config::default()
    };
    config.enumeration.rename_variants = RenameRule::QualifiedScreamingSnakeCase;

    let mut rename = HashMap::new();
    for name in MOBILE_TYPES {
        rename.insert((*name).to_owned(), format!("Whisker{name}"));
    }
    for (rust, c) in CALLBACKS {
        rename.insert((*rust).to_owned(), (*c).to_owned());
    }
    for constant in public_constants(&source) {
        let c_name = match constant.as_str() {
            "BACKGROUND_REPEAT_REPEAT" => "WHISKER_BACKGROUND_REPEAT".into(),
            "BACKGROUND_REPEAT_NO_REPEAT" => "WHISKER_BACKGROUND_NO_REPEAT".into(),
            "BACKGROUND_REPEAT_SPACE" => "WHISKER_BACKGROUND_SPACE".into(),
            "BACKGROUND_REPEAT_ROUND" => "WHISKER_BACKGROUND_ROUND".into(),
            _ => format!("WHISKER_{constant}"),
        };
        rename.insert(constant, c_name);
    }
    config.export.rename = rename;
    config
        .export
        .exclude
        .push("whisker_mobile_bridge_anchor".into());
    config.export.include = MOBILE_TYPES
        .iter()
        .copied()
        .chain(CALLBACKS.iter().map(|(rust, _)| *rust))
        .chain([
            "WhiskerStringRef",
            "WhiskerBytesRef",
            "WhiskerValueArray",
            "WhiskerValueMap",
            "WhiskerValueUnion",
            "WhiskerValueRaw",
            "WhiskerKeyValueRaw",
        ])
        .map(str::to_owned)
        .collect();

    let bindings = cbindgen::Builder::new()
        .with_config(config)
        .with_crate(crate_root)
        .generate()
        .context("generate mobile ABI header")?;
    let mut output = Vec::new();
    bindings.write(&mut output);
    let output = String::from_utf8(output).context("cbindgen emitted non-UTF-8 header")?;
    Ok(GeneratedAbi {
        header: order_recursive_value_types(output)?.into_bytes(),
        kotlin: kotlin_constants(&source).into_bytes(),
        android_jni_header: android_jni_header().into_bytes(),
        android_jni_kotlin: android_jni_kotlin().into_bytes(),
    })
}

fn write_generated(root: &Path, relative: &str, contents: &[u8]) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, contents).with_context(|| format!("write {}", path.display()))
}

fn check_generated(root: &Path, relative: &str, expected: &[u8]) -> Result<()> {
    let path = root.join(relative);
    let checked_in = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    if checked_in != expected {
        bail!(
            "{} is stale; run `cargo xtask mobile-abi generate`",
            path.display()
        );
    }
    Ok(())
}

fn android_jni_header() -> String {
    let mut output = String::from(
        "/* Generated by `cargo xtask mobile-abi generate`; do not edit. */\n\
         #ifndef WHISKER_ANDROID_JNI_H_\n\
         #define WHISKER_ANDROID_JNI_H_\n\n\
         #define WHISKER_ANDROID_OPERATION_STRIDE 10\n",
    );
    for (index, name) in ANDROID_OPERATION_FIELDS.iter().enumerate() {
        output.push_str(&format!(
            "#define WHISKER_ANDROID_OPERATION_{name} {index}\n"
        ));
    }
    output.push_str("\n#endif\n");
    output
}

fn android_jni_kotlin() -> String {
    let mut output = String::from(
        "// Generated by `cargo xtask mobile-abi generate`; do not edit.\n\
         package rs.whisker.runtime.bridge\n\n\
         internal object AndroidFrameBatch {\n",
    );
    output.push_str("    const val STRIDE: Int = 10\n");
    for (index, name) in ANDROID_OPERATION_FIELDS.iter().enumerate() {
        output.push_str(&format!("    const val {name}: Int = {index}\n"));
    }
    output.push_str("}\n");
    output
}

fn order_recursive_value_types(mut header: String) -> Result<String> {
    // cbindgen cannot topologically order this legal C cycle on its own:
    // Raw -> union -> map pointer -> key/value -> Raw by value. Move the one
    // by-value entry after Raw while retaining the generated declarations.
    let key_start = header
        .find("/**\n * String-keyed map entry.\n */\ntypedef struct WhiskerKeyValueRaw")
        .context("find generated WhiskerKeyValueRaw")?;
    let key_end = header[key_start..]
        .find("} WhiskerKeyValueRaw;\n")
        .map(|offset| key_start + offset + "} WhiskerKeyValueRaw;\n".len())
        .context("find end of generated WhiskerKeyValueRaw")?;
    let key_value = header[key_start..key_end].to_owned();
    header.replace_range(key_start..key_end, "");
    let raw_end = header
        .find("} WhiskerValueRaw;\n")
        .map(|offset| offset + "} WhiskerValueRaw;\n".len())
        .context("find generated WhiskerValueRaw")?;
    header.insert_str(raw_end, &format!("\n{key_value}"));
    Ok(header)
}

fn public_constants(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("pub const "))
        .filter_map(|declaration| declaration.split_once(':').map(|(name, _)| name.to_owned()))
        .collect()
}

fn kotlin_constants(source: &str) -> String {
    let constants = source.lines().filter_map(|line| {
        let declaration = line.trim_start().strip_prefix("pub const ")?;
        let (name, rest) = declaration.split_once(':')?;
        let (_, value) = rest.split_once('=')?;
        Some((name.trim(), value.trim().trim_end_matches(';')))
    });
    let mut output = String::from(
        "// Generated by `cargo xtask mobile-abi generate`; do not edit.\n\
         package rs.whisker.runtime.bridge\n\n\
         internal object MobileAbi {\n",
    );
    for (name, value) in constants {
        output.push_str(&format!("    const val {name}: Int = {value}\n"));
    }
    output.push_str("}\n");
    output
}

fn layout_assertions() -> &'static str {
    r#"
#ifndef WHISKER_MOBILE_ABI_ASSERTIONS_
#define WHISKER_MOBILE_ABI_ASSERTIONS_
#if defined(__cplusplus)
#define WHISKER_ABI_STATIC_ASSERT(condition, message) static_assert(condition, message)
#else
#define WHISKER_ABI_STATIC_ASSERT(condition, message) _Static_assert(condition, message)
#endif
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerValueRaw) == 24, "WhiskerValueRaw ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerKeyValueRaw) == 40, "WhiskerKeyValueRaw ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileOperation) == 72, "WhiskerMobileOperation ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileFrame) == 72, "WhiskerMobileFrame ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileMeasureRequest) == 224, "WhiskerMobileMeasureRequest ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileMeasureResponse) == 64, "WhiskerMobileMeasureResponse ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileText) == 248, "WhiskerMobileText ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileBoxPaint) == 272, "WhiskerMobileBoxPaint ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileResourceCommand) == 64, "WhiskerMobileResourceCommand ABI drift");
WHISKER_ABI_STATIC_ASSERT(sizeof(WhiskerMobileResourceEvent) == 56, "WhiskerMobileResourceEvent ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerValueRaw, type) == 0, "WhiskerValueRaw.type ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerValueRaw, v) == 8, "WhiskerValueRaw.v ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerKeyValueRaw, value) == 16, "WhiskerKeyValueRaw.value ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerMobileOperation, node) == 8, "WhiskerMobileOperation.node ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerMobileOperation, integer) == 40, "WhiskerMobileOperation.integer ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerMobileOperation, payload) == 56, "WhiskerMobileOperation.payload ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerMobileFrame, surface) == 16, "WhiskerMobileFrame.surface ABI drift");
WHISKER_ABI_STATIC_ASSERT(offsetof(WhiskerMobileFrame, operations) == 56, "WhiskerMobileFrame.operations ABI drift");
#undef WHISKER_ABI_STATIC_ASSERT
#endif
"#
}
