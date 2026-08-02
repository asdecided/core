//! Mutation-boundary tests for `decided rename`.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use rac_engine::output::render_rename_human;
    use rac_engine::rename::{apply_rename, compute_rename, REASON_SYMLINK_PATH};

    const DECISION: &str = "---\nschema_version: 1\nid: RAC-111111111111\ntype: decision\n---\n# Rename boundary\n\n## Context\n\nThe path must stay inside the corpus.\n\n## Decision\n\nReject symlinked mutation paths.\n\n## Consequences\n\nExternal targets remain unchanged.\n\n## Status\n\nAccepted\n";

    fn scratch_root() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "asdecided-rename-boundary-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn dry_run_rejects_symlink_and_apply_cannot_touch_external_target() {
        let base = scratch_root();
        let root = base.join("corpus");
        let outside = base.join("outside");
        fs::create_dir_all(&root).expect("create scratch corpus");
        fs::create_dir_all(&outside).expect("create scratch dirs");
        let external = outside.join("decision.md");
        let linked = root.join("decision.md");
        fs::write(&external, DECISION).expect("write external decision");
        symlink(&external, &linked).expect("create corpus symlink");

        let root_text = root.to_string_lossy().to_string();
        let plan = compute_rename(&root_text, "RAC-111111111111", "RAC-222222222222", true);

        assert!(!plan.ok);
        assert_eq!(plan.reason, Some(REASON_SYMLINK_PATH));
        assert_eq!(plan.target_path.as_deref(), Some(linked.to_str().unwrap()));
        let human = render_rename_human(&plan);
        assert!(human.contains("symlink"), "{human}");
        assert!(human.contains(linked.to_str().unwrap()), "{human}");
        let result = apply_rename(&plan).expect("refused plans are not errors");
        assert!(!result.applied);
        assert_eq!(fs::read_to_string(&external).unwrap(), DECISION);

        fs::remove_dir_all(base).expect("remove scratch corpus");
    }

    #[test]
    fn apply_rechecks_path_before_replacement() {
        let base = scratch_root();
        let root = base.join("corpus");
        fs::create_dir_all(&root).expect("create scratch corpus");
        let target = root.join("decision.md");
        let outside = base.join("external.md");
        fs::write(&target, DECISION).expect("write corpus decision");
        fs::write(&outside, DECISION).expect("write external decision");

        let root_text = root.to_string_lossy().to_string();
        let plan = compute_rename(&root_text, "RAC-111111111111", "RAC-222222222222", true);
        assert!(plan.ok, "unexpected refusal: {:?}", plan.reason);

        fs::remove_file(&target).expect("remove original target");
        symlink(&outside, &target).expect("swap target for symlink");
        let error = match apply_rename(&plan) {
            Ok(_) => panic!("symlink swap must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), DECISION);

        fs::remove_dir_all(base).expect("remove scratch corpus");
    }
}
