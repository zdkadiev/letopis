use super::confined::{
    ConfinedParent, get_stat, map_io, require_identity, require_type, verify_entry,
};
use crate::service::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use rustix::fs::{FileType, Mode, OFlags, Stat, fstat, openat};
use std::fs::File;
use std::io::Read;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadStage {
    BeforeOpen,
    AfterRead,
}

pub(super) fn get_regular(
    root: &File,
    path: &str,
    mut observe: impl FnMut(ReadStage),
) -> EmbeddingResult<Option<Vec<u8>>> {
    let parts: Vec<_> = path.split('/').collect();
    let (name, ancestors) = parts
        .split_last()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::Input))?;
    let Some(parent) = ConfinedParent::walk(root, ancestors)? else {
        return Ok(None);
    };
    let Some(before) = get_stat(&parent.directory, name)? else {
        parent.verify_links()?;
        return Ok(None);
    };
    require_type(&before, FileType::RegularFile)?;
    observe(ReadStage::BeforeOpen);
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let mut file =
        File::from(openat(&parent.directory, *name, flags, Mode::empty()).map_err(map_io)?);
    let opened = fstat(&file).map_err(map_io)?;
    require_type(&opened, FileType::RegularFile)?;
    require_unchanged(&before, &opened)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(map_io)?;
    observe(ReadStage::AfterRead);
    require_unchanged(&opened, &fstat(&file).map_err(map_io)?)?;
    require_unchanged(&opened, &verify_entry(&parent.directory, name, &opened)?)?;
    parent.verify_links()?;
    Ok(Some(bytes))
}

fn require_unchanged(before: &Stat, after: &Stat) -> EmbeddingResult<()> {
    require_identity(before, after)?;
    if before.st_size != after.st_size
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
        || before.st_ctime != after.st_ctime
        || before.st_ctime_nsec != after.st_ctime_nsec
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::Input));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::require_unchanged;
    use crate::service::EmbeddingErrorKind;
    use rustix::fs::fstat;
    use std::fs::File;

    #[test]
    fn rejects_each_independent_stat_change() {
        // Given: initialize platform-specific fields through a real descriptor.
        let file = File::open(".").unwrap();
        let before = fstat(&file).unwrap();
        let mut changes = [before; 5];
        changes[0].st_size += 1;
        changes[1].st_mtime += 1;
        changes[2].st_mtime_nsec += 1;
        changes[3].st_ctime += 1;
        changes[4].st_ctime_nsec += 1;
        // When / Then
        assert!(require_unchanged(&before, &before).is_ok());
        for after in changes {
            let error = require_unchanged(&before, &after).unwrap_err();
            assert_eq!(error.get_kind(), EmbeddingErrorKind::Input);
        }
    }
}
