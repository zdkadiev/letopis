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
