use crate::StorageError;
use serde::{Deserialize, Serialize};

pub const MANIFEST_NAME: &str = "rit-storage.manifest.json";
pub const IDENTITY_TABLE_SQL: &str = "CREATE TABLE rit_storage_identity (k TEXT PRIMARY KEY NOT NULL, v TEXT NOT NULL CHECK(length(v)>0))";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestState {
    InitInProgress,
    Ready,
    LegacyAdoptionInProgress,
    AdoptionReady,
    RecoveryRequired,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub manifest_version: u32,
    pub state: ManifestState,
    pub application_id: i64,
    pub user_version: i64,
    pub db_uuid: String,
    pub generation_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_serial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

pub fn random_token_hex() -> Result<String, StorageError> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|_| StorageError::UnsafeDirectory("CSPRNG unavailable"))?;
    let mut s = String::with_capacity(64);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    Ok(s)
}

pub fn new_db_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
