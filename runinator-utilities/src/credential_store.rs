use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use runinator_models::errors::SendableError;
use serde::{Deserialize, Serialize};

pub trait CredentialStore: Send + Sync {
    fn put(&self, scope: &str, name: &str, secret: &[u8]) -> Result<(), SendableError>;
    fn get(&self, scope: &str, name: &str) -> Result<Option<Vec<u8>>, SendableError>;
    fn delete(&self, scope: &str, name: &str) -> Result<(), SendableError>;
}

#[derive(Debug, Clone)]
pub struct LocalEncryptedCredentialStore {
    path: PathBuf,
    key: Vec<u8>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialFile {
    entries: BTreeMap<String, String>,
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
    fn put(&self, scope: &str, name: &str, secret: &[u8]) -> Result<(), SendableError> {
        let mut file = self.load()?;
        file.entries.insert(
            Self::entry_key(scope, name),
            hex_encode(&self.crypt(secret)),
        );
        self.save(&file)
    }

    fn get(&self, scope: &str, name: &str) -> Result<Option<Vec<u8>>, SendableError> {
        let file = self.load()?;
        let Some(raw) = file.entries.get(&Self::entry_key(scope, name)) else {
            return Ok(None);
        };
        Ok(Some(self.crypt(&hex_decode(raw)?)))
    }

    fn delete(&self, scope: &str, name: &str) -> Result<(), SendableError> {
        let mut file = self.load()?;
        file.entries.remove(&Self::entry_key(scope, name));
        self.save(&file)
    }
}

pub fn default_credential_store_path(base: impl AsRef<Path>) -> PathBuf {
    base.as_ref().join("credentials.enc.json")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

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
