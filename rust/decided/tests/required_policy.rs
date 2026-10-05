use std::{fs, process::Command};

#[test]
fn cli_required_policy_fails_before_evaluation_and_reloads_after_removal() {
    let root = std::env::temp_dir().join(format!("decided-required-cli-{}", std::process::id()));
    fs::create_dir_all(root.join(".decided")).unwrap();
    fs::create_dir_all(root.join("decisions")).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_decided"))
            .args(["gate", "decisions", "--require-policy"])
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let missing = run();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("required policy absent"));
    fs::write(root.join(".decided/config.yaml"), "enforcement: {}\n").unwrap();
    assert!(run().status.success());
    fs::remove_file(root.join(".decided/config.yaml")).unwrap();
    assert_eq!(run().status.code(), Some(1));
    fs::remove_dir_all(root).unwrap();
}
