use std::{collections::BTreeSet, fs};

fn resource_keys(path: &str) -> BTreeSet<String> {
    println!("cargo:rerun-if-changed={path}");
    let source = fs::read_to_string(path).expect("registration UI resource must be UTF-8");
    let mut keys = BTreeSet::new();
    for line in source.lines() {
        let (key, value) = line
            .split_once('=')
            .expect("registration UI resource requires key=value");
        assert!(
            !key.is_empty() && key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "invalid registration UI resource key"
        );
        assert!(
            !value.trim().is_empty() && !value.chars().any(char::is_control),
            "invalid registration UI resource value"
        );
        assert!(
            keys.insert(key.to_owned()),
            "duplicate registration UI resource key"
        );
    }
    assert!(!keys.is_empty(), "registration UI resource cannot be empty");
    keys
}
fn main() {
    assert_eq!(
        resource_keys("resources/en.txt"),
        resource_keys("resources/ko.txt"),
        "registration UI languages must contain the same keys"
    );
    assert_eq!(
        resource_keys("resources/setup-en.txt"),
        resource_keys("resources/setup-ko.txt"),
        "setup UI languages must contain the same keys"
    );
}
