use rac_engine::gate::{
    build_gate, policy_load_state, require_enforcement_policy, PolicyLoadState,
};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("decided-policy-{}-{id}", std::process::id()));
        fs::create_dir_all(path.join(".decided")).unwrap();
        fs::create_dir_all(path.join("decisions")).unwrap();
        Self(path)
    }
    fn directory(&self) -> String {
        self.0.join("decisions").display().to_string()
    }
    fn config(&self) -> PathBuf {
        self.0.join(".decided/config.yaml")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn optional_absence_uses_defaults_but_required_absence_blocks() {
    let f = Fixture::new();
    assert_eq!(policy_load_state(&f.directory()), PolicyLoadState::Absent);
    assert!(build_gate(&f.directory(), true).is_ok());
    assert!(require_enforcement_policy(&f.directory()).is_err());
    fs::write(f.config(), "enforcement: {}\n").unwrap();
    assert_eq!(policy_load_state(&f.directory()), PolicyLoadState::Loaded);
    assert!(require_enforcement_policy(&f.directory()).is_ok());
    fs::remove_file(f.config()).unwrap();
    assert!(require_enforcement_policy(&f.directory()).is_err());
}

#[test]
fn malformed_policy_blocks_at_gate_and_diagnostics_do_not_echo_yaml() {
    let f = Fixture::new();
    for text in [
        "secret-value",
        "[]",
        "enforcement: [secret-value]",
        "enforcement: [\nsecret-value",
    ] {
        fs::write(f.config(), text).unwrap();
        assert_eq!(policy_load_state(&f.directory()), PolicyLoadState::Invalid);
        let error = build_gate(&f.directory(), true).err().unwrap();
        assert!(!error.message().contains("secret-value"));
    }
}

#[test]
fn required_policy_needs_enforcement_section() {
    let f = Fixture::new();
    for text in ["validation: {}", "enforcement: null"] {
        fs::write(f.config(), text).unwrap();
        assert!(require_enforcement_policy(&f.directory()).is_err());
    }
}

#[test]
fn directory_at_config_path_is_not_optional_absence() {
    let f = Fixture::new();
    fs::create_dir(f.config()).unwrap();
    assert_eq!(
        policy_load_state(&f.directory()),
        PolicyLoadState::Unreadable
    );
    assert!(build_gate(&f.directory(), true).is_err());
}

#[cfg(unix)]
#[test]
fn dangling_symlink_cannot_fall_through_to_parent_policy() {
    let f = Fixture::new();
    fs::write(f.config(), "enforcement: {}\n").unwrap();
    fs::create_dir(f.0.join("decisions/.decided")).unwrap();
    std::os::unix::fs::symlink("missing", f.0.join("decisions/.decided/config.yaml")).unwrap();
    assert_eq!(
        policy_load_state(&f.directory()),
        PolicyLoadState::Unreadable
    );
    assert!(build_gate(&f.directory(), true).is_err());
    assert!(require_enforcement_policy(&f.directory()).is_err());
}
