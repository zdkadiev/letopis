use super::fail_integrity;
use crate::{
    domain::{SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN},
    service::SnapshotError,
};
use sha1_checked::Sha1;
use sha2::{Digest, Sha256};

pub(super) enum ObjectHasher {
    Sha1(Box<Sha1>),
    Sha256(Sha256),
}
impl ObjectHasher {
    pub(super) fn new(oid_len: usize, kind: &str, size: u64) -> Result<Self, SnapshotError> {
        let mut hash = match oid_len {
            SHA1_OID_HEX_LEN => Self::Sha1(Box::new(Sha1::new())),
            SHA256_OID_HEX_LEN => Self::Sha256(Sha256::new()),
            _ => return Err(fail_integrity()),
        };
        hash.update(format!("{kind} {size}\0").as_bytes());
        Ok(hash)
    }
    pub(super) fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha1(hash) => hash.update(bytes),
            Self::Sha256(hash) => hash.update(bytes),
        }
    }
    pub(super) fn finish(self) -> Result<String, SnapshotError> {
        match self {
            Self::Sha1(hash) => {
                let result = hash.try_finalize();
                if result.has_collision() {
                    return Err(fail_integrity());
                }
                Ok(format!("{:x}", result.hash()))
            }
            Self::Sha256(hash) => Ok(format!("{:x}", hash.finalize())),
        }
    }
}
