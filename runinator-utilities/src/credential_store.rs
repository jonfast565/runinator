use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use runinator_models::errors::SendableError;
use serde::{Deserialize, Serialize};

use crate::app_data;

pub trait CredentialStore: Send + Sync {
    /// store a secret, stamping it with the current time.
    fn put(&self, scope: &str, name: &str, secret: &[u8]) -> Result<(), SendableError> {
        self.put_at(scope, name, secret, now_unix())
    }
    /// store a secret with an explicit modification time (unix seconds).
    fn put_at(
        &self,
        scope: &str,
        name: &str,
        secret: &[u8],
        updated_at: i64,
    ) -> Result<(), SendableError>;
    fn get(&self, scope: &str, name: &str) -> Result<Option<Vec<u8>>, SendableError>;
    /// modification time (unix seconds) of a stored secret, or None when it does not exist.
    fn entry_updated_at(&self, scope: &str, name: &str) -> Result<Option<i64>, SendableError>;
    fn delete(&self, scope: &str, name: &str) -> Result<(), SendableError>;
    fn list(&self) -> Result<Vec<CredentialEntry>, SendableError>;
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialEntry {
    pub scope: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct LocalEncryptedCredentialStore {
    path: PathBuf,
    key: Vec<u8>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialFile {
    entries: BTreeMap<String, StoredSecret>,
}

// stored entry format. legacy files hold a bare hex string with no timestamp;
// new writes are objects carrying the modification time. untagged deserialization
// accepts both so existing credential files keep working.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum StoredSecret {
    Legacy(String),
    Versioned {
        secret: String,
        #[serde(default)]
        updated_at: i64,
    },
}

impl StoredSecret {
    fn secret_hex(&self) -> &str {
        match self {
            StoredSecret::Legacy(secret) => secret,
            StoredSecret::Versioned { secret, .. } => secret,
        }
    }

    // legacy entries predate timestamps, so treat them as the epoch (oldest).
    fn updated_at(&self) -> i64 {
        match self {
            StoredSecret::Legacy(_) => 0,
            StoredSecret::Versioned { updated_at, .. } => *updated_at,
        }
    }
}

impl LocalEncryptedCredentialStore {
    pub fn new(path: impl Into<PathBuf>, key: impl AsRef<[u8]>) -> Self {
        Self {
            path: path.into(),
            key: key.as_ref().to_vec(),
        }
    }

    fn entry_key(scope: &str, name: &str) -> String {
        format!("{scope}:{name}")
    }

    fn load(&self) -> Result<CredentialFile, SendableError> {
        if !self.path.exists() {
            return Ok(CredentialFile::default());
        }
        let raw = fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save(&self, file: &CredentialFile) -> Result<(), SendableError> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_vec_pretty(file)?)?;
        Ok(())
    }

    fn crypt(&self, input: &[u8]) -> Vec<u8> {
        if self.key.is_empty() {
            return input.to_vec();
        }
        input
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.key[index % self.key.len()])
            .collect()
    }
}

impl CredentialStore for LocalEncryptedCredentialStore {
    fn put_at(
        &self,
        scope: &str,
        name: &str,
        secret: &[u8],
        updated_at: i64,
    ) -> Result<(), SendableError> {
        let mut file = self.load()?;
        file.entries.insert(
            Self::entry_key(scope, name),
            StoredSecret::Versioned {
                secret: hex_encode(&self.crypt(secret)),
                updated_at,
            },
        );
        self.save(&file)
    }

    fn get(&self, scope: &str, name: &str) -> Result<Option<Vec<u8>>, SendableError> {
        let file = self.load()?;
        let Some(stored) = file.entries.get(&Self::entry_key(scope, name)) else {
            return Ok(None);
        };
        Ok(Some(self.crypt(&hex_decode(stored.secret_hex())?)))
    }

    fn entry_updated_at(&self, scope: &str, name: &str) -> Result<Option<i64>, SendableError> {
        let file = self.load()?;
        Ok(file
            .entries
            .get(&Self::entry_key(scope, name))
            .map(StoredSecret::updated_at))
    }

    fn delete(&self, scope: &str, name: &str) -> Result<(), SendableError> {
        let mut file = self.load()?;
        file.entries.remove(&Self::entry_key(scope, name));
        self.save(&file)
    }

    fn list(&self) -> Result<Vec<CredentialEntry>, SendableError> {
        let file = self.load()?;
        Ok(file
            .entries
            .keys()
            .filter_map(|key| {
                let (scope, name) = key.split_once(':')?;
                Some(CredentialEntry {
                    scope: scope.to_string(),
                    name: name.to_string(),
                })
            })
            .collect())
    }
}

pub fn default_credential_store_path(base: impl AsRef<Path>) -> PathBuf {
    base.as_ref().join("credentials.enc.json")
}

pub fn default_app_credential_store_path() -> Result<PathBuf, SendableError> {
    Ok(default_credential_store_path(app_data::app_data_dir()?))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "credential_store_tests.rs"]
mod tests;

fn hex_decode(raw: &str) -> Result<Vec<u8>, SendableError> {
    if raw.len() % 2 != 0 {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "hex credential payload has odd length",
        )));
    }
    let mut bytes = Vec::with_capacity(raw.len() / 2);
    for index in (0..raw.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&raw[index..index + 2], 16)?);
    }
    Ok(bytes)
}
