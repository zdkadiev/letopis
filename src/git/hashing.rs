use super::constants::OBJECT_INTEGRITY_MESSAGE;
use crate::{
    domain::{SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN},
    service::{SnapshotError, SnapshotErrorKind},
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
            _ => {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    OBJECT_INTEGRITY_MESSAGE,
                ));
            }
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
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InternalError,
                        OBJECT_INTEGRITY_MESSAGE,
                    ));
                }
                Ok(format!("{:x}", result.hash()))
            }
            Self::Sha256(hash) => Ok(format!("{:x}", hash.finalize())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectHasher, Sha1};
    use sha2::Digest;
    #[test]
    fn rejects_sha1_collision() {
        // Given: the upstream unprefixed SHA-mbles collision block.
        let mut raw = Sha1::new();
        raw.update(include_bytes!("../../tests/fixtures/git/sha-mbles-1.bin"));
        // When: the existing hasher finalizes the detected collision.
        let result = ObjectHasher::Sha1(Box::new(raw)).finish();
        // Then: collision detection propagates as an integrity error.
        assert!(result.is_err());
    }
}
