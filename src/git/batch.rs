use super::hashing::ObjectHasher;
use super::{
    constants::{BLOB_KIND, TREE_KIND},
    process::{GitProcess, make_exit_error, make_io_error},
};
use crate::service::{SnapshotError, SnapshotErrorKind};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout},
    thread::{self, JoinHandle},
};

pub(super) mod tree;
const MAX_HEADER_BYTES: u64 = 256;
const BUFFER_BYTES: usize = 64 * 1024;

pub(super) struct BatchReader {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr: Option<JoinHandle<io::Result<u64>>>,
    oid_len: usize,
}
#[derive(Debug)]
pub(super) enum VerifiedObject {
    Blob,
    Tree(Vec<u8>),
}
impl BatchReader {
    #[expect(
        clippy::expect_used,
        reason = "GitProcess::build_command always pipes all three streams"
    )]
    pub(super) fn open(
        process: &GitProcess,
        root: &Path,
        oid_len: usize,
    ) -> Result<Self, SnapshotError> {
        let mut child = process
            .build_command(root, &["cat-file".into(), "--batch".into()])
            .spawn()
            .map_err(|e| make_io_error("Git batch spawn failed", e))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut stderr = child.stderr.take().expect("piped stderr");
        let drainer = thread::spawn(move || io::copy(&mut stderr, &mut io::sink()));
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout,
            stderr: Some(drainer),
            oid_len,
        })
    }

    pub(super) fn read_object(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<VerifiedObject, SnapshotError> {
        if oid.len() != self.oid_len
            || !oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(fail_integrity());
        }
        let stdin = self.stdin.as_mut().ok_or_else(fail_integrity)?;
        writeln!(stdin, "{oid}")
            .and_then(|()| stdin.flush())
            .map_err(|e| make_io_error("Git batch stdin failed", e))?;
        let size = self.read_header(oid, expected, missing, wrong_type)?;
        self.read_body(oid, expected, size)
    }

    pub(super) fn finish(mut self) -> Result<(), SnapshotError> {
        self.stdin.take();
        let mut trailing = [0];
        if self
            .stdout
            .read(&mut trailing)
            .map_err(|e| make_io_error("Git batch stdout failed", e))?
            != 0
        {
            return Err(fail_integrity());
        }
        let status = self
            .child
            .wait()
            .map_err(|e| make_io_error("Git batch wait failed", e))?;
        if !status.success() {
            return Err(make_exit_error("Git batch command failed", status));
        }
        self.join_stderr()
    }

    fn read_header(
        &mut self,
        oid: &str,
        expected: &str,
        missing: SnapshotErrorKind,
        wrong_type: SnapshotErrorKind,
    ) -> Result<u64, SnapshotError> {
        let mut header = Vec::new();
        Read::take(&mut self.stdout, MAX_HEADER_BYTES)
            .read_until(b'\n', &mut header)
            .map_err(|e| make_io_error("Git batch header failed", e))?;
        let header = std::str::from_utf8(&header)
            .map_err(|_| fail_integrity())?
            .strip_suffix('\n')
            .ok_or_else(fail_integrity)?;
        if header == format!("{oid} missing") {
            return Err(SnapshotError::new(missing, "Git object is missing"));
        }
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || fields[0] != oid
            || fields[2].is_empty()
            || !fields[2].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(fail_integrity());
        }
        if fields[1] != expected {
            return Err(SnapshotError::new(wrong_type, "unexpected Git object type"));
        }
        fields[2].parse().map_err(|_| fail_integrity())
    }

    fn read_body(
        &mut self,
        oid: &str,
        kind: &str,
        size: u64,
    ) -> Result<VerifiedObject, SnapshotError> {
        if kind != BLOB_KIND && kind != TREE_KIND {
            return Err(fail_integrity());
        }
        let mut hash = ObjectHasher::new(self.oid_len, kind, size)?;
        let mut tree = Vec::new();
        let mut remaining = size;
        let mut buffer = [0; BUFFER_BYTES];
        while remaining != 0 {
            let count = usize::try_from(remaining.min(BUFFER_BYTES as u64))
                .map_err(|_| fail_integrity())?;
            let bytes = &mut buffer[..count];
            self.stdout
                .read_exact(bytes)
                .map_err(|e| make_io_error("Git batch body failed", e))?;
            hash.update(bytes);
            if kind == TREE_KIND {
                tree.try_reserve(count).map_err(|_| fail_integrity())?;
                tree.extend_from_slice(bytes);
            }
            remaining -= u64::try_from(count).map_err(|_| fail_integrity())?;
        }
        let mut trailer = [0];
        self.stdout
            .read_exact(&mut trailer)
            .map_err(|e| make_io_error("Git batch trailer failed", e))?;
        if trailer != [b'\n'] || hash.finish()? != oid {
            return Err(fail_integrity());
        }
        Ok(if kind == TREE_KIND {
            VerifiedObject::Tree(tree)
        } else {
            VerifiedObject::Blob
        })
    }

    fn join_stderr(&mut self) -> Result<(), SnapshotError> {
        if let Some(worker) = self.stderr.take() {
            worker
                .join()
                .map_err(|_| fail_integrity())?
                .map_err(|e| make_io_error("Git batch stderr failed", e))?;
        }
        Ok(())
    }
}
impl Drop for BatchReader {
    fn drop(&mut self) {
        self.stdin.take();
        // Errors and early drops cannot leave a child blocked on either pipe.
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.join_stderr();
    }
}
fn fail_integrity() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git object integrity failed",
    )
}
#[cfg(test)]
mod tests;
