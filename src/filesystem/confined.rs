use crate::service::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use crate::utils::has_same_identity;
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, openat, statat};
use rustix::io::Errno;
use std::fs::File;

pub(super) const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Each link retains its parent so post-I/O checks observe ancestor replacement.
struct DirectoryLink {
    parent: File,
    name: String,
    identity: Stat,
}
pub(super) struct ConfinedParent {
    pub(super) directory: File,
    links: Vec<DirectoryLink>,
}
impl ConfinedParent {
    pub(super) fn walk(root: &File, components: &[&str]) -> EmbeddingResult<Option<Self>> {
        let mut parent = Self {
            directory: root.try_clone().map_err(map_io)?,
            links: Vec::new(),
        };
        for &name in components {
            let Some(before) = read_stat(&parent.directory, name)? else {
                parent.verify_links()?;
                return Ok(None);
            };
            require_type(&before, FileType::Directory)?;
            let next = File::from(
                openat(&parent.directory, name, DIRECTORY_FLAGS, Mode::empty()).map_err(map_io)?,
            );
            require_identity(&before, &fstat(&next).map_err(map_io)?)?;
            let previous = std::mem::replace(&mut parent.directory, next);
            parent.links.push(DirectoryLink {
                parent: previous,
                name: name.into(),
                identity: before,
            });
        }
        parent.verify_links()?;
        Ok(Some(parent))
    }

    pub(super) fn verify_links(&self) -> EmbeddingResult<()> {
        for link in &self.links {
            verify_entry(&link.parent, &link.name, &link.identity)?;
        }
        Ok(())
    }
}

pub(super) fn read_stat(parent: &File, name: &str) -> EmbeddingResult<Option<Stat>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(Errno::NOENT) => match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(None),
            Err(error) => Err(map_io(error)),
            Ok(_) => Err(reject_input()),
        },
        Err(error) => Err(map_io(error)),
    }
}

pub(super) fn verify_entry(parent: &File, name: &str, identity: &Stat) -> EmbeddingResult<Stat> {
    let current = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(map_io)?;
    require_identity(identity, &current)?;
    Ok(current)
}
pub(super) fn require_identity(before: &Stat, after: &Stat) -> EmbeddingResult<()> {
    if !has_same_identity(before, after) || before.st_mode != after.st_mode {
        return Err(reject_input());
    }
    Ok(())
}
pub(super) fn require_type(stat: &Stat, expected: FileType) -> EmbeddingResult<()> {
    if FileType::from_raw_mode(stat.st_mode) != expected {
        return Err(reject_input());
    }
    Ok(())
}
pub(super) fn reject_input() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::Input)
}
pub(super) fn map_io(error: impl std::error::Error + Send + Sync + 'static) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
}
