use std::{
    io::{self, IsTerminal, Read, Stdin},
    os::fd::AsRawFd,
};

const FIRST_BYTE_WAIT_MS: i32 = 250;

/// Reads process stdin, treating a silent pipe as an empty request.
#[derive(Debug)]
pub(crate) struct ProcessInput {
    stdin: Stdin,
    is_started: bool,
    is_empty: bool,
}

impl ProcessInput {
    /// Creates a reader that skips terminal input and bounds the first pipe read.
    ///
    /// # Arguments
    ///
    /// * `stdin` — the process stdin to read; when it is a terminal, reads immediately
    ///   report empty input.
    pub(crate) fn new(stdin: Stdin) -> Self {
        let is_empty = stdin.is_terminal();
        Self {
            stdin,
            is_started: false,
            is_empty,
        }
    }
}

impl Read for ProcessInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.is_empty || buf.is_empty() {
            return Ok(0);
        }
        if !self.is_started {
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
                self.is_empty = true;
                return Ok(0);
            }
            self.is_started = true;
        }
        self.stdin.read(buf)
    }
}
