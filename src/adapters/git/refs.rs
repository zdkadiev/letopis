use super::{GitCliAdapter, objects::ObjectDb, process::is_exit_code};
use crate::{
    domain::TreeOid,
    service::ports::git::{RetainedTree, SnapshotError, SnapshotErrorKind::InternalError},
};
use std::collections::BTreeMap;
pub(super) struct Retainer<'a> {
    pub(super) git: &'a GitCliAdapter,
}
impl Retainer<'_> {
    pub(super) fn pin(&self, oid: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        let db = ObjectDb { git: self.git };
        db.validate_tree(oid)?;
        let mut pending = vec![(oid.as_str().to_owned(), "tree".to_owned())];
        let mut visited = BTreeMap::new();
        while let Some((oid, kind)) = pending.pop() {
            if let Some(previous) = visited.insert(oid.clone(), kind.clone()) {
                if previous != kind {
                    return Err(SnapshotError::new(
                        InternalError,
                        "conflicting Git object types",
                    ));
                }
                continue;
            }
            if kind == "tree" {
                for entry in db.entries(&oid)? {
                    if entry.kind != "gitlink" {
                        pending.push((entry.oid, entry.kind));
                    }
                }
            } else {
                db.verified(&oid, "blob", InternalError, InternalError)?;
            }
        }
        let name = format!("refs/letopis/snapshots/{}", oid.as_str());
        if self.exact_ref(&db, &name, oid)? {
            return Ok(RetainedTree::new(oid.clone()));
        }
        let zero = "0".repeat(self.git.oid_len);
        match db.run(
            &["update-ref", "--no-deref", &name, oid.as_str(), &zero],
            None,
        ) {
            Ok(_) => {}
            Err(cause) => {
                if !self.exact_ref(&db, &name, oid)? {
                    return Err(cause);
                }
            }
        }
        if !self.exact_ref(&db, &name, oid)? {
            return Err(SnapshotError::new(
                InternalError,
                "snapshot ref was not confirmed",
            ));
        }
        Ok(RetainedTree::new(oid.clone()))
    }
    // Only an exact, direct ref to this exact tree is idempotent success.
    fn exact_ref(
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
                    InternalError,
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
                    InternalError,
                    "invalid snapshot ref response",
                ));
            }
            if fields[0] == name.as_bytes() {
                if fields[1] != oid.as_str().as_bytes() || !fields[2].is_empty() {
                    return Err(SnapshotError::new(InternalError, "snapshot ref conflicts"));
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}
