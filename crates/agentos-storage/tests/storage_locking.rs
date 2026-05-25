use agentos_storage::{
    AgentOsStorage, StorageError, StorageLockGuard, StorageLockInfo, StorageLockOptions,
};
use chrono::{Duration, Utc};

fn options(owner_id: &str) -> StorageLockOptions {
    StorageLockOptions::new(owner_id, Duration::minutes(5))
}

fn read_lock(path: &std::path::Path) -> StorageLockInfo {
    let bytes = std::fs::read(path.join(".agentos-storage.lock")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn acquire_lock_creates_lock_file_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();

    let guard = StorageLockGuard::acquire(dir.path(), options("owner-a")).unwrap();

    assert!(guard.lock_path().is_file());
    let info = read_lock(dir.path());
    assert_eq!(info.lock_version, 1);
    assert_eq!(info.owner_id, "owner-a");
    assert_eq!(info.process_id, std::process::id());
    assert!(info.expires_at > info.acquired_at);
    assert_eq!(guard.info(), &info);
}

#[test]
fn second_instance_cannot_acquire_active_lock() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let _guard = StorageLockGuard::acquire(dir.path(), options("owner-a")).unwrap();

    let result = StorageLockGuard::acquire(dir.path(), options("owner-b"));

    assert!(matches!(
        result.unwrap_err(),
        StorageError::LockAlreadyHeld { owner_id, .. } if owner_id == "owner-a"
    ));
}

#[test]
fn dropping_guard_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let lock_path = dir.path().join(".agentos-storage.lock");

    {
        let _guard = StorageLockGuard::acquire(dir.path(), options("owner-a")).unwrap();
        assert!(lock_path.is_file());
    }

    assert!(!lock_path.exists());
    let _guard = StorageLockGuard::acquire(dir.path(), options("owner-b")).unwrap();
    assert!(lock_path.is_file());
}

#[test]
fn explicit_release_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let lock_path = dir.path().join(".agentos-storage.lock");
    let guard = StorageLockGuard::acquire(dir.path(), options("owner-a")).unwrap();
    assert!(lock_path.is_file());

    guard.release().unwrap();

    assert!(!lock_path.exists());
}

#[test]
fn stale_lock_can_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let lock_path = dir.path().join(".agentos-storage.lock");
    let now = Utc::now();
    let stale = StorageLockInfo {
        lock_version: 1,
        owner_id: "stale-owner".to_string(),
        process_id: 999_999,
        acquired_at: now - Duration::minutes(10),
        expires_at: now - Duration::minutes(5),
    };
    std::fs::write(&lock_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();

    let guard = StorageLockGuard::acquire(dir.path(), options("fresh-owner")).unwrap();

    assert_eq!(guard.info().owner_id, "fresh-owner");
    let info = read_lock(dir.path());
    assert_eq!(info.owner_id, "fresh-owner");
    assert!(info.expires_at > Utc::now());
}

#[test]
fn two_storage_instances_enforce_single_writer_guard() {
    let dir = tempfile::tempdir().unwrap();
    let storage_a = AgentOsStorage::init(dir.path()).unwrap();
    let storage_b = AgentOsStorage::init(dir.path()).unwrap();

    let guard_a = storage_a.acquire_lock(options("owner-a")).unwrap();
    let result = storage_b.acquire_lock(options("owner-b"));

    assert!(matches!(
        result.unwrap_err(),
        StorageError::LockAlreadyHeld { owner_id, .. } if owner_id == "owner-a"
    ));

    drop(guard_a);
    let guard_b = storage_b.acquire_lock(options("owner-b")).unwrap();
    assert_eq!(guard_b.info().owner_id, "owner-b");
}

#[test]
fn guard_does_not_remove_lock_replaced_by_other_owner() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let lock_path = dir.path().join(".agentos-storage.lock");
    let guard = StorageLockGuard::acquire(dir.path(), options("owner-a")).unwrap();
    let now = Utc::now();
    let replacement = StorageLockInfo {
        lock_version: 1,
        owner_id: "owner-b".to_string(),
        process_id: std::process::id(),
        acquired_at: now,
        expires_at: now + Duration::minutes(5),
    };
    std::fs::write(&lock_path, serde_json::to_vec_pretty(&replacement).unwrap()).unwrap();

    drop(guard);

    assert!(lock_path.is_file());
    assert_eq!(read_lock(dir.path()).owner_id, "owner-b");
}
