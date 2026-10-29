use rustix::process::{Pid, Signal, kill_process_group};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CHILD_CASE: &str = "LETOPIS_BATCH_PROTOCOL_CHILD";
struct OwnedTest(Child);

pub(super) fn supervise(name: &str) -> bool {
    if std::env::var(CHILD_CASE).ok().as_deref() == Some(name) {
        return false;
    }
    let mut output = tempfile::tempfile().unwrap();
    let mut child = OwnedTest(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CHILD_CASE, name)
            .process_group(0)
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "batch protocol child exceeded deadline: {name}"
        );
        thread::sleep(Duration::from_millis(5));
    };
    output.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    output.read_to_string(&mut text).unwrap();
    assert!(status.success(), "{text}");
    assert!(text.contains("test result: ok. 1 passed;"), "{text}");
    true
}
impl Drop for OwnedTest {
    fn drop(&mut self) {
        let pid = Pid::from_raw(i32::try_from(self.0.id()).unwrap()).unwrap();
        let _ = kill_process_group(pid, Signal::KILL);
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
