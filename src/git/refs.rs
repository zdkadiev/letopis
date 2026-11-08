use super::{
    GitRepository,
    constants::{BLOB_KIND, TREE_KIND},
    durability::{ref_lock_is_busy, ref_retain_by_oid},
    objects::tree_validate_by_oid,
    reader::VerifiedObject,
    tree::tree_entry_get,
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

/// Verifies the root and durably confirms its exact direct ref without loading descendants.
///
/// # Arguments
///
/// * `git` — repository that verifies the root and retains the snapshot ref.
/// * `oid` — identity of the root tree to validate and retain.
pub(super) fn tree_retain_by_oid(git: &GitRepository, oid: &TreeOid) -> Result<(), SnapshotError> {
    tree_validate_by_oid(git, oid)?;
    tree_ref_retain_by_oid(git, oid)
}

/// Verifies the entire closure on every call and then durably retains its root.
///
/// # Arguments
///
/// * `git` — repository that verifies the closure and retains the snapshot ref.
/// * `oid` — identity of the root tree to verify and pin.
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

/// Verifies every object reachable from `root`, ignoring gitlinks, and rejects type conflicts.
///
/// # Arguments
///
/// * `git` — repository whose reader and object format verify the closure.
/// * `root` — identity of the root tree whose reachable objects are checked.
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

/// Returns the direct child object identities and kinds from a verified tree body.
///
/// # Arguments
///
/// * `bytes` — verified tree body whose direct children are decoded.
/// * `oid_len` — hexadecimal object ID length used to frame each entry.
///
/// # Errors
/// Returns `InternalError` for malformed entry boundaries or octal modes.
pub(super) fn tree_children_get(
    mut bytes: &[u8],
    oid_len: usize,
) -> Result<Vec<(String, String)>, SnapshotError> {
    let mut children = Vec::new();
    while !bytes.is_empty() {
        let (entry, remaining) = tree_entry_get(bytes, oid_len)?;
        let kind = match entry.mode & 0o170000 {
            0o100000 | 0o120000 => Some(BLOB_KIND),
            0o040000 => Some(TREE_KIND),
            _ => None,
        };
        if let Some(kind) = kind {
            let oid: String = entry.oid.iter().map(|byte| format!("{byte:02x}")).collect();
            children.push((oid, kind.into()));
        }
        bytes = remaining;
    }
    Ok(children)
}
