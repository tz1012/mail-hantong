//! Shared build-time linkage for native UI binaries and their test harnesses.
pub fn embed_common_controls_manifest() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/hantong-secrets/resources/common-controls.manifest");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-changed=../../scripts/windows_manifest.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let path = path.canonicalize().expect("native UI manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", path.display());
    }
}
