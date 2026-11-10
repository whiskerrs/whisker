use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(whisker_host_lib)");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=WHISKER_HOST_LIB_DIR");
    println!("cargo::rerun-if-env-changed=WHISKER_APP_SDK_BOOTSTRAP_LIB");
    let (Some(host), Some(bootstrap)) = (
        std::env::var_os("WHISKER_HOST_LIB_DIR"),
        std::env::var_os("WHISKER_APP_SDK_BOOTSTRAP_LIB"),
    ) else {
        return;
    };
    let bootstrap = PathBuf::from(bootstrap);
    println!("cargo::rustc-link-search=native={}", host.display());
    println!(
        "cargo::rustc-link-search=native={}",
        bootstrap.parent().unwrap().display()
    );
    for library in [
        "WhiskerHostConformance",
        "WhiskerHost",
        bootstrap.file_stem().unwrap().to_str().unwrap(),
        "windowsapp",
        "user32",
    ] {
        println!("cargo::rustc-link-lib={library}");
    }
    println!("cargo::rustc-cfg=whisker_host_lib");
}
