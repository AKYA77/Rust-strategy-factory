use std::{fs, process::Command};
#[test]
fn actual_cli_backup_verify_restore_and_path_keep_primary_inactive_and_private() {
    let root = std::env::temp_dir().join(format!("recovery-cli-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let db = root.join("state.sqlite");
    drop(rust_strategy_factory::storage::Registry::open(&db).unwrap());
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let command = |args: &[&str]| {
        let out = Command::new(binary).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    command(&["registry-check", db.to_str().unwrap()]);
    assert_eq!(
        command(&["recovery-list", db.to_str().unwrap()])["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let one = command(&["recovery-backup", db.to_str().unwrap(), "cli-one"]);
    assert_eq!(one["fresh_content_verified"], true);
    assert!(!one.to_string().contains(root.to_str().unwrap()));
    assert_eq!(
        command(&["recovery-verify", db.to_str().unwrap(), "cli-one"])["manifest_hash"],
        one["manifest_hash"]
    );
    let copied = command(&[
        "recovery-restore",
        db.to_str().unwrap(),
        "cli-one",
        "new-copy",
    ]);
    assert_eq!(copied["activation_performed"], false);
    assert_eq!(copied["parent_id"], "cli-one");
    let path = command(&["recovery-path", db.to_str().unwrap(), "new-copy"]);
    assert!(std::path::Path::new(path["path"].as_str().unwrap()).is_file());
    assert_eq!(
        command(&["recovery-list", db.to_str().unwrap()])["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !Command::new(binary)
            .args(["recovery-backup", db.to_str().unwrap(), "cli-one"])
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::remove_dir_all(root).unwrap();
}
