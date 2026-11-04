use super::{
    GitRepository,
    constants::TREE_KIND,
    durability::{ref_lock_is_busy, ref_retain_by_oid},
    objects::tree_validate_by_oid,
    reader::VerifiedObject,
    tree::tree_children_get,
    validation::tree_bytes_validate,
};
use crate::{
    domain::TreeOid,
    service::{RetainedTree, SnapshotError, SnapshotErrorKind},
};
use gix::hash::ObjectId;
use std::{collections::BTreeMap, thread, time::Duration};
// Retry safe lock contention through the entire confirmation protocol; not a command deadline.
const REF_LOCK_ATTEMPTS: usize = 32;
const REF_RETRY_DELAY: Duration = Duration::from_millis(10);
/// Verify the root and durably confirm its exact direct ref, without loading descendants.
pub(super) fn tree_retain_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    tree_ref_retain_by_oid(git, oid)
}
/// Revalidate the entire closure on every call before durably retaining its root.
pub(super) fn tree_pin_by_oid(
    git: &GitRepository,
    oid: &TreeOid,
) -> Result<RetainedTree, SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    tree_verify_closure_by_oid(git, oid)?;
    tree_ref_retain_by_oid(git, oid)?;
    Ok(RetainedTree::new(oid.clone()))
}
fn tree_ref_retain_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    let oid = ObjectId::from_hex(oid.as_str().as_bytes()).map_err(|_| {
        SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git identity")
    })?;
    for attempt in 0..REF_LOCK_ATTEMPTS {
        match ref_retain_by_oid(&git.repo, oid, &git.cleanup) {
            Err(error) if ref_lock_is_busy(&error) && attempt + 1 < REF_LOCK_ATTEMPTS => {
                thread::sleep(REF_RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("bounded retry always returns on the last attempt")
}
/// Verify every object reachable from `root`, ignoring gitlinks, and reject type conflicts.
fn tree_verify_closure_by_oid(git: &GitRepository, root: &TreeOid) -> Result<(), SnapshotError> {
    let mut reader = git.reader.borrow_mut();
    let mut pending = vec![(root.as_str().to_owned(), TREE_KIND.to_owned())];
    let mut visited = BTreeMap::new();
    while let Some((oid, kind)) = pending.pop() {
        if let Some(previous) = visited.insert(oid.clone(), kind.clone()) {
            if previous != kind {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "conflicting Git object types",
                ));
            }
            continue;
        }
        let object = reader.object_get_by_oid(
            &oid,
            &kind,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        if let VerifiedObject::Tree(bytes) = object {
            tree_bytes_validate(&bytes, git.oid_len)?;
            pending.extend(tree_children_get(&bytes, git.oid_len)?);
        }
    }
    Ok(())
}
