use std::{
    io::{self, IsTerminal, Read, Stdin},
    os::fd::AsRawFd,
};

const FIRST_BYTE_WAIT_MS: i32 = 250;

/// Reads process stdin, treating a silent pipe as an empty request.
#[derive(Debug)]
pub(crate) struct ProcessInput {
    stdin: Stdin,
    started: bool,
    empty: bool,
}

impl ProcessInput {
    /// Create a reader that skips terminal input and bounds the first pipe read.
    pub(crate) fn new(stdin: Stdin) -> Self {
        let empty = stdin.is_terminal();
        Self {
            stdin,
            started: false,
            empty,
        }
    }
}

impl Read for ProcessInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.empty || buf.is_empty() {
            return Ok(0);
        }
        if !self.started {
            let mut fd = libc::pollfd {
                fd: self.stdin.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: fd points to a valid local pollfd for the duration of this call.
            let ready = unsafe { libc::poll(&raw mut fd, 1, FIRST_BYTE_WAIT_MS) };
            if ready < 0 {
                return Err(io::Error::last_os_error());
            }
            if ready == 0 {
                self.empty = true;
                return Ok(0);
            }
            self.started = true;
        }
        self.stdin.read(buf)
    }
}
