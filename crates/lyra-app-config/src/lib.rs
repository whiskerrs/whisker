//! App configuration types used by `lyra.rs`.
//!
//! Users build an `AppConfig` via the builder API:
//! ```ignore
//! pub fn configure(app: &mut AppConfig) {
//!     app.name("MyApp")
//!        .bundle_id("dev.example.myapp")
//!        .version("1.0.0");
//!
//!     app.ios(|ios| ios.deployment_target("13.0"));
//!     app.android(|android| android.min_sdk(24));
//! }
//! ```

#[derive(Debug, Default)]
pub struct AppConfig {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<u32>,
    pub ios: IosConfig,
    pub android: AndroidConfig,
}

impl AppConfig {
    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = Some(name.into());
        self
    }

    pub fn bundle_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.bundle_id = Some(id.into());
        self
    }

    pub fn version(&mut self, v: impl Into<String>) -> &mut Self {
        self.version = Some(v.into());
        self
    }

    pub fn build_number(&mut self, n: u32) -> &mut Self {
        self.build_number = Some(n);
        self
    }

    pub fn ios(&mut self, f: impl FnOnce(&mut IosConfig)) -> &mut Self {
        f(&mut self.ios);
        self
    }

    pub fn android(&mut self, f: impl FnOnce(&mut AndroidConfig)) -> &mut Self {
        f(&mut self.android);
        self
    }
}

#[derive(Debug, Default)]
pub struct IosConfig {
    pub deployment_target: Option<String>,
}

impl IosConfig {
    pub fn deployment_target(&mut self, t: impl Into<String>) -> &mut Self {
        self.deployment_target = Some(t.into());
        self
    }
}

#[derive(Debug, Default)]
pub struct AndroidConfig {
    pub package: Option<String>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
}

impl AndroidConfig {
    pub fn package(&mut self, p: impl Into<String>) -> &mut Self {
        self.package = Some(p.into());
        self
    }

    pub fn min_sdk(&mut self, v: u32) -> &mut Self {
        self.min_sdk = Some(v);
        self
    }

    pub fn target_sdk(&mut self, v: u32) -> &mut Self {
        self.target_sdk = Some(v);
        self
    }
}
