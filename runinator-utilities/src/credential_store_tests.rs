use std::time::{SystemTime, UNIX_EPOCH};

use crate::credential_store::{CredentialStore, LocalEncryptedCredentialStore};

fn temp_store_path(label: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("runinator-credstore-{label}-{unique}.json"))
}

#[test]
fn put_at_persists_secret_and_timestamp() {
    let path = temp_store_path("put-at");
    let store = LocalEncryptedCredentialStore::new(&path, "key");

    store
        .put_at("github", "main", b"token", 1_700_000_000)
        .unwrap();

    assert_eq!(
        store.get("github", "main").unwrap().as_deref(),
        Some(&b"token"[..])
    );
    assert_eq!(
        store.entry_updated_at("github", "main").unwrap(),
        Some(1_700_000_000)
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn put_stamps_current_time() {
    let path = temp_store_path("put-now");
    let store = LocalEncryptedCredentialStore::new(&path, "key");

    store.put("github", "main", b"token").unwrap();

    let stamped = store.entry_updated_at("github", "main").unwrap().unwrap();
    assert!(stamped > 0);
    let _ = std::fs::remove_file(path);
}

#[test]
fn entry_updated_at_is_none_for_missing_secret() {
    let path = temp_store_path("missing");
    let store = LocalEncryptedCredentialStore::new(&path, "key");

    assert_eq!(store.entry_updated_at("github", "main").unwrap(), None);
    let _ = std::fs::remove_file(path);
}

#[test]
fn reads_legacy_bare_string_entries_as_epoch() {
    let path = temp_store_path("legacy");
    let store = LocalEncryptedCredentialStore::new(&path, "");
    // write a legacy file whose entries are bare hex strings with no timestamp.
    // with an empty key the stored value is the plain hex of the secret bytes.
    let legacy = serde_json::json!({ "entries": { "github:main": "746f6b656e" } });
    std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

    assert_eq!(
        store.get("github", "main").unwrap().as_deref(),
        Some(&b"token"[..])
    );
    // legacy entries predate timestamps and reconcile as the oldest possible.
    assert_eq!(store.entry_updated_at("github", "main").unwrap(), Some(0));
    let _ = std::fs::remove_file(path);
}
