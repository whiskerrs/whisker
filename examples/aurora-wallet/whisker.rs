// `whisker.rs` for the `aurora-wallet` example.
//
// A self-contained, single-`lib.rs` finance dashboard. No native
// modules, no bundled assets — just the core runtime — so it builds
// and runs from one file and is ideal for demoing the hot-reload loop.

pub fn configure(app: &mut whisker_config::Config) {
    app.name("Aurora Wallet")
        .bundle_id("rs.whisker.aurorawallet")
        .version("0.1.0")
        .build_number(1);

    app.android(|a| {
        a.package("rs.whisker.aurorawallet")
            .application_id("rs.whisker.aurorawallet")
            .launcher_activity(".MainActivity")
            .min_sdk(24)
            .target_sdk(34);
    });

    app.ios(|i| {
        i.bundle_id("rs.whisker.aurorawallet")
            .scheme("AuroraWallet")
            .deployment_target("13.0");
    });
}
