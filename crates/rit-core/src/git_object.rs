//! Algorithm-aware Git object identifiers and read-only ref/history observations.
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Git object hashing algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectFormat {
    /// SHA-1 repository (40 hexadecimal digits).
    Sha1,
    /// SHA-256 repository (64 hexadecimal digits).
    Sha256,
}
impl ObjectFormat {
    /// Parse Git's object format name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            _ => None,
        }
    }
    fn hex_len(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}
/// A complete, validated object ID and its repository's object format.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct ObjectId {
    algorithm: ObjectFormat,
    hex: String,
}
impl ObjectId {
    /// Validate a complete lowercase hexadecimal OID against the algorithm.
    pub fn new(algorithm: ObjectFormat, hex: &str) -> Result<Self, &'static str> {
        if hex.len() != algorithm.hex_len()
            || !hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("invalid full Git object ID");
        }
        Ok(Self {
            algorithm,
            hex: hex.to_owned(),
        })
    }
    /// The object format.
    pub fn algorithm(&self) -> ObjectFormat {
        self.algorithm
    }
    /// The full lowercase hexadecimal identifier.
    pub fn as_hex(&self) -> &str {
        &self.hex
    }
}
impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.hex.fmt(f)
    }
}
impl<'de> Deserialize<'de> for ObjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            algorithm: ObjectFormat,
            hex: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.algorithm, &wire.hex).map_err(serde::de::Error::custom)
    }
}
/// A ref reported by Git, not a durable identity or a synchronization guarantee.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitReference {
    /// Full reference name.
    pub name: String,
    /// Direct target object.
    pub object: ObjectId,
    /// Git-reported direct object type, e.g. commit or tag.
    pub object_type: String,
    /// Target of an annotated tag when Git reports one.
    pub peeled: Option<ObjectId>,
}
/// A read-only commit observation, without author, message or file content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    /// Commit ID.
    pub id: ObjectId,
    /// Parent IDs in Git-reported order; history may be shallow.
    pub parents: Vec<ObjectId>,
    /// Git committer time in Unix seconds, not a trusted ordering source.
    pub committed_at: i64,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_full_oid_and_serde() {
        let s = ObjectId::new(ObjectFormat::Sha1, &"a".repeat(40)).unwrap();
        assert_eq!(s.algorithm(), ObjectFormat::Sha1);
        assert!(ObjectId::new(ObjectFormat::Sha1, &"a".repeat(64)).is_err());
        assert!(ObjectId::new(ObjectFormat::Sha256, &"g".repeat(64)).is_err());
        assert!(ObjectId::new(ObjectFormat::Sha1, &"A".repeat(40)).is_err());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<ObjectId>(&json).unwrap(), s);
        assert!(
            serde_json::from_str::<ObjectId>(r#"{"algorithm":"sha1","hex":"not-an-id"}"#).is_err()
        );
    }
}
