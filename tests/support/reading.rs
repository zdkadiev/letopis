use super::git::{ObjectFormat, OwnedChild, TempRepo, isolate_command};
use std::{
    fs,
    io::{Seek, Write},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub struct ReadingRepo(TempRepo);
impl ReadingRepo {
    pub fn new() -> Self {
        Self(TempRepo::new(ObjectFormat::Sha1))
    }
    pub fn get_root(&self) -> &Path {
        self.0.get_root()
    }
    pub fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.get_root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    pub fn invoke(&self, args: &[&str], input: &[u8]) -> Output {
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(input).unwrap();
        stdin.rewind().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
        isolate_command(&mut command);
        command
            .current_dir(self.get_root())
            .args(args)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30))
    }
}
