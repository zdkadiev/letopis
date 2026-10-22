use crate::domain::TreeOid;

/// A tree whose complete local closure has a confirmed permanent service ref.
#[derive(Debug)]
pub struct RetainedTree {
    oid: TreeOid,
}

impl RetainedTree {
    pub(crate) fn new(oid: TreeOid) -> Self {
        Self { oid }
    }
    /// The exact retained tree OID.
    pub fn get_oid(&self) -> &TreeOid {
        &self.oid
    }
}
