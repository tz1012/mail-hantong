use std::{collections::BTreeSet, fs};

fn keys(path: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for line in fs::read_to_string(path).expect("resource file").lines() {
        let (key, value) = line.split_once('=').expect("resource key=value");
        assert!(!key.is_empty() && !value.is_empty(), "empty core resource");
        assert!(keys.insert(key.to_owned()), "duplicate core resource");
    }
    keys
}
fn main() {
    let en = keys("resources/en.txt");
    assert_eq!(en, keys("resources/ko.txt"));
    for key in [
        "usage",
        "description",
        "invalid_input",
        "already_running",
        "unsupported",
        "unavailable",
    ] {
        assert!(en.contains(key), "missing core resource");
    }
    println!("cargo:rerun-if-changed=resources/en.txt");
    println!("cargo:rerun-if-changed=resources/ko.txt");
}
