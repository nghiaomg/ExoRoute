use super::*;

#[test]
fn stale_temp_cleanup_removes_only_owned_backup_directories() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-temp-cleanup-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated temp folder");
    let stale_import = root.join("lmdb-import-stale");
    let stale_backup = root.join("lmdb-backup-stale");
    let unrelated = root.join("user-data");
    fs::create_dir(&stale_import).expect("create stale import folder");
    fs::create_dir(&stale_backup).expect("create stale backup folder");
    fs::create_dir(&unrelated).expect("create unrelated folder");

    assert_eq!(
        cleanup_stale_temp_directories(&root).expect("clean stale dirs"),
        2
    );
    assert!(!stale_import.exists());
    assert!(!stale_backup.exists());
    assert!(unrelated.exists());
    fs::remove_dir_all(root).expect("remove isolated test folder");
}

#[tokio::test]
async fn temp_directory_drop_queues_cleanup_off_runtime_workers() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-temp-drop-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated temp folder");
    drop(TempDirectory(root.clone()));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !tokio::fs::try_exists(&root)
                .await
                .expect("inspect temp folder")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bounded cleanup worker removes dropped temp directory");
}
