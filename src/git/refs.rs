use super::{
    GitCli,
    batch::{BatchReader, VerifiedObject},
    constants::{SNAPSHOT_REF_PREFIX, TREE_KIND, UPDATE_REF_COMMAND},
    objects::ObjectDb,
    process::is_exit_code,
    tree::read_children,
    validation::validate_tree_bytes,
};
use crate::{
    domain::TreeOid,
    service::{RetainedTree, SnapshotError, SnapshotErrorKind},
};
use std::collections::BTreeMap;
pub(super) struct Retainer<'a> {
    pub(super) git: &'a GitCli,
}
impl Retainer<'_> {
    pub(super) fn retain(&self, oid: &TreeOid) -> Result<(), SnapshotError> {
        let db = ObjectDb { git: self.git };
        db.validate_tree(oid)?;
        self.retain_ref(&db, oid)
    }
    pub(super) fn pin(&self, oid: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        let db = ObjectDb { git: self.git };
        db.validate_tree(oid)?;
        verify_closure(&db, oid)?;
        self.retain_ref(&db, oid)?;
        Ok(RetainedTree::new(oid.clone()))
    }
    fn retain_ref(&self, db: &ObjectDb<'_>, oid: &TreeOid) -> Result<(), SnapshotError> {
        let name = format!("{SNAPSHOT_REF_PREFIX}{}", oid.as_str());
        if !self.has_exact_ref(db, &name, oid)? {
            self.publish_ref(db, &name, oid)?;
        }
        Ok(())
    }
    // A concurrent writer may publish the same ref first; that is success once confirmed.
    fn publish_ref(
        &self,
        db: &ObjectDb<'_>,
        name: &str,
        oid: &TreeOid,
    ) -> Result<(), SnapshotError> {
        let zero = "0".repeat(self.git.oid_len);
        if let Err(cause) = db.run(
            &[UPDATE_REF_COMMAND, "--no-deref", name, oid.as_str(), &zero],
            None,
        ) && !self.has_exact_ref(db, name, oid)?
        {
            return Err(cause);
        }
        if !self.has_exact_ref(db, name, oid)? {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "snapshot ref was not confirmed",
            ));
        }
        Ok(())
    }
    // Only an exact, direct ref to this exact tree is idempotent success.
    fn has_exact_ref(
        &self,
        db: &ObjectDb<'_>,
        name: &str,
        oid: &TreeOid,
    ) -> Result<bool, SnapshotError> {
        // for-each-ref omits dangling symbolic refs. Probe first so zero-OID
        // CAS cannot silently replace one of them. Exit 1 alone means non-symbolic.
        match db.run(&["symbolic-ref", "-q", name], None) {
            Ok(_) => {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "snapshot ref is symbolic",
                ));
            }
            Err(error) if is_exit_code(&error, 1) => {}
            Err(error) => return Err(error),
        }
        let raw = db.run(
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(symref)%00",
                name,
            ],
            None,
        )?;
        for record in raw.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
            let fields: Vec<_> = record.split(|b| *b == 0).collect();
            if fields.len() != 4 {
                return Err(SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    "invalid snapshot ref response",
                ));
            }
            if fields[0] == name.as_bytes() {
                if fields[1] != oid.as_str().as_bytes() || !fields[2].is_empty() {
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InternalError,
                        "snapshot ref conflicts",
                    ));
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Verify every object reachable from `root`, ignoring gitlinks, and reject type conflicts.
fn verify_closure(db: &ObjectDb<'_>, root: &TreeOid) -> Result<(), SnapshotError> {
    let mut reader = BatchReader::open(&db.git.process, &db.git.root, db.git.oid_len)?;
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
        let object = reader.read_object(
            &oid,
            &kind,
            SnapshotErrorKind::InternalError,
            SnapshotErrorKind::InternalError,
        )?;
        if let VerifiedObject::Tree(bytes) = object {
            validate_tree_bytes(&bytes, db.git.oid_len)?;
            pending.extend(read_children(&bytes, db.git.oid_len)?);
        }
    }
    reader.finish()
}
