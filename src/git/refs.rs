use super::{
    GitRepository,
    constants::TREE_KIND,
    durability::{is_ref_lock_busy, retain_ref},
    objects::ObjectDb,
    reader::VerifiedObject,
    tree::get_children,
    validation::validate_tree_bytes,
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
/// Keep lightweight root retention distinct from full closure verification.
#[derive(Debug)]
pub(super) struct Retainer<'a> {
    pub(super) git: &'a GitRepository,
}
impl Retainer<'_> {
    /// Verify the root and durably confirm its exact direct ref, without loading descendants.
    pub(super) fn retain(&self, oid: &TreeOid) -> Result<(), SnapshotError> {
        ObjectDb { git: self.git }.validate_tree(oid)?;
        self.retain_ref(oid)
    }
    /// Revalidate the entire closure on every call before durably retaining its root.
    pub(super) fn pin(&self, oid: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        let db = ObjectDb { git: self.git };
        db.validate_tree(oid)?;
        verify_closure(&db, oid)?;
        self.retain_ref(oid)?;
        Ok(RetainedTree::new(oid.clone()))
    }
    fn retain_ref(&self, oid: &TreeOid) -> Result<(), SnapshotError> {
        let oid = ObjectId::from_hex(oid.as_str().as_bytes()).map_err(|_| {
            SnapshotError::new(SnapshotErrorKind::InternalError, "invalid Git identity")
        })?;
        for attempt in 0..REF_LOCK_ATTEMPTS {
            match retain_ref(&self.git.repo, oid, &self.git.cleanup) {
                Err(error) if is_ref_lock_busy(&error) && attempt + 1 < REF_LOCK_ATTEMPTS => {
                    thread::sleep(REF_RETRY_DELAY);
                }
                result => return result,
            }
        }
        unreachable!("bounded retry always returns on the last attempt")
    }
}
/// Verify every object reachable from `root`, ignoring gitlinks, and reject type conflicts.
fn verify_closure(db: &ObjectDb<'_>, root: &TreeOid) -> Result<(), SnapshotError> {
    let mut reader = db.git.reader.borrow_mut();
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
        let object = reader.get_object(
            &oid,
            &kind,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        if let VerifiedObject::Tree(bytes) = object {
            validate_tree_bytes(&bytes, db.git.oid_len)?;
            pending.extend(get_children(&bytes, db.git.oid_len)?);
        }
    }
    Ok(())
}
