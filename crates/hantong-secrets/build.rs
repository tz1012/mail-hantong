#[path = "../../scripts/windows_manifest.rs"]
mod windows_manifest;
fn main() {
    windows_manifest::embed_common_controls_manifest();
}
