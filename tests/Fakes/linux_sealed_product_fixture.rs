#![cfg(target_os = "linux")]

use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const VERSION: &str = "0.1.0-test";
const COMMIT: &str = "a0123456789abcdef0123456789abcdef0123456";
const SOURCE_HASH: &str = "b0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde";
const LOCK_HASH: &str = "c0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde";

pub struct ProductFixture {
    base: PathBuf,
    binary: PathBuf,
}

impl ProductFixture {
    pub fn new(asset: &str, binary_source: &str) -> Self {
        assert!(matches!(
            asset,
            "hantong" | "hantong-mcp" | "mail-hantong-core"
        ));
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "hantong-r2-product-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&base).expect("fixture base");
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("fixture mode");
        let root = base.join("versions").join(VERSION);
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("fixture release");
        fs::set_permissions(root.parent().unwrap(), fs::Permissions::from_mode(0o700))
            .expect("versions mode");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("release mode");
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).expect("bin mode");
        let source = PathBuf::from(binary_source);
        let data = fs::read(&source).expect("read test binary");
        let binary = bin.join(asset);
        fs::write(&binary, &data).expect("copy test binary");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("binary mode");
        assert!(
            Command::new("/usr/bin/strip")
                .args(["--strip-debug", binary.to_str().expect("binary path")])
                .status()
                .expect("strip test image")
                .success(),
            "strip test image failed"
        );
        let binary_hash = sha256_file(&binary);
        let binary_size = fs::metadata(&binary).expect("binary metadata").len();
        assert!(
            binary_size <= 64 * 1024 * 1024,
            "stripped test image exceeds the production release bound"
        );
        write_json(
            &root.join("release-manifest.json"),
            &json!({"format_version":1,"version":VERSION,"package_version":"0.1.0","target":"linux-x86_64","files":[{"path":format!("bin/{asset}"),"size":binary_size,"sha256":binary_hash}]}),
        );
        write_json(
            &root.join("BUILD-METADATA.json"),
            &json!({"source_commit":COMMIT,"source_hash":SOURCE_HASH,"cargo_lock_sha256":LOCK_HASH,"build_performed":true}),
        );
        let descriptor = json!({"schema_version":1,"version":VERSION,"package_version":"0.1.0","target":"linux-x86_64","commit":COMMIT,"source_filename":format!("mail-hantong-source-{VERSION}.zip"),"cargo_lock_sha256":LOCK_HASH,"source_sha256":SOURCE_HASH,"source_archive_sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","package_filename":format!("mail-hantong-cli-{VERSION}-linux-x86_64.tar.gz"),"package_sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"});
        let evidence = base.join("release-pins").join(VERSION).join(COMMIT);
        fs::create_dir_all(&evidence).expect("fixture evidence");
        fs::set_permissions(
            evidence.parent().unwrap().parent().unwrap(),
            fs::Permissions::from_mode(0o700),
        )
        .expect("pins mode");
        fs::set_permissions(
            evidence.parent().unwrap(),
            fs::Permissions::from_mode(0o700),
        )
        .expect("version evidence mode");
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).expect("evidence mode");
        write_json(&evidence.join("descriptor.json"), &descriptor);
        write_json(
            &evidence.join("pin.json"),
            &json!({"format_version":1,"target":"linux-x86_64","version":VERSION,"commit":COMMIT,"descriptor_sha256":sha256_file(&evidence.join("descriptor.json"))}),
        );
        Self { base, binary }
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .env("XDG_DATA_HOME", self.private_dir("data-home"))
            .env("XDG_CONFIG_HOME", self.private_dir("config-home"))
            .env("XDG_RUNTIME_DIR", self.private_dir("runtime"));
        command
    }

    pub fn private_dir(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir_all(&path).expect("fixture private directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("fixture private directory mode");
        path
    }
}
impl Drop for ProductFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).expect("fixture json")).expect("fixture file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("fixture file mode");
}
fn sha256_file(path: &Path) -> String {
    let output = Command::new("/usr/bin/sha256sum")
        .arg(path)
        .output()
        .expect("sha256sum");
    assert!(output.status.success(), "sha256sum failed");
    String::from_utf8(output.stdout)
        .expect("sha256sum utf8")
        .split_whitespace()
        .next()
        .expect("sha256sum output")
        .to_owned()
}
