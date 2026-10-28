use super::ResponseWriter;
use crate::{
    cli::{CommandResponse, ResponseResult},
    service::{CommandError, CommandErrorKind, CommandResult},
};
use std::{
    io::{self, Write},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

mod signals;

const WAIT: Duration = Duration::from_secs(10);
const SHORT_WAIT: Duration = Duration::from_millis(200);
const CANCELLED_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";
const SUCCESS_ENVELOPE: &[u8] = b"{\"ok\":true,\"result\":{}}\n";

#[test]
fn cancellation_winner_prevents_success_exit() {
    // Given: a cancellation response pauses while it owns output.
    let writer = Arc::new(ResponseWriter::default());
    let cancellation_writer = Arc::clone(&writer);
    let PausedOutput {
        writer: mut output,
        bytes,
        entered,
        resume,
    } = PausedOutput::new(Pause::Write);
    let cancellation = thread::spawn(move || {
        cancellation_writer.write_response(&mut output, cancelled_response())
    });
    assert_eq!(entered.recv_timeout(WAIT).unwrap(), Pause::Write);
    // When: a successful response races for the same single output.
    let success_writer = Arc::clone(&writer);
    let (done, completed) = mpsc::channel();
    let normal = thread::spawn(move || {
        let mut output = Vec::new();
        let exit = success_writer.write_response(&mut output, successful_response());
        done.send((exit, output)).unwrap();
    });
    assert!(completed.recv_timeout(SHORT_WAIT).is_err());
    resume.send(()).unwrap();
    let cancellation_exit = cancellation.join().unwrap();
    let (normal_exit, normal_output) = completed.recv_timeout(WAIT).unwrap();
    normal.join().unwrap();
    // Then: the cancellation envelope is the only output, and its exit code wins.
    assert_eq!(cancellation_exit, 1);
    assert_eq!(&*bytes.lock().unwrap(), CANCELLED_ENVELOPE);
    assert_eq!(normal_exit, 1);
    assert!(normal_output.is_empty());
}

#[test]
fn normal_winner_prevents_second_json() {
    // Given: a normal response has completed successfully.
    let writer = ResponseWriter::default();
    let mut first_output = Vec::new();
    // When: a cancellation response attempts to write through the same owner.
    let first_exit = writer.write_response(&mut first_output, successful_response());
    let mut second_output = Vec::new();
    let second_exit = writer.write_response(&mut second_output, cancelled_response());
    // Then: exactly one success envelope exists and the loser receives its exit code.
    assert_eq!(first_exit, 0);
    assert_eq!(first_output, SUCCESS_ENVELOPE);
    assert_eq!(second_exit, 0);
    assert!(second_output.is_empty());
}

#[test]
fn write_failure_never_retries_output() {
    // Given: output fails while writing the first response.
    let writer = ResponseWriter::default();
    let attempts = Arc::new(Mutex::new(0));
    let mut failing = FailingWriter {
        attempts: Arc::clone(&attempts),
    };
    // When: a later cancellation response is offered the same owner.
    let first_exit = writer.write_response(&mut failing, successful_response());
    let mut second_output = Vec::new();
    let second_exit = writer.write_response(&mut second_output, cancelled_response());
    // Then: no second output is attempted after the failed one.
    assert_eq!(first_exit, 1);
    assert_eq!(*attempts.lock().unwrap(), 1);
    assert_eq!(second_exit, 1);
    assert!(second_output.is_empty());
}

#[test]
fn guard_covers_flush() {
    // Given: a success response is paused inside flush while holding the guard.
    let writer = Arc::new(ResponseWriter::default());
    let first_writer = Arc::clone(&writer);
    let PausedOutput {
        writer: mut first_output,
        bytes,
        entered,
        resume,
    } = PausedOutput::new(Pause::Flush);
    let first = thread::spawn(move || {
        first_writer.write_response(&mut first_output, successful_response())
    });
    assert_eq!(entered.recv_timeout(WAIT).unwrap(), Pause::Flush);
    // When: a cancellation response races while the first flush is still pending.
    let second_writer = Arc::clone(&writer);
    let (done, completed) = mpsc::channel();
    let second = thread::spawn(move || {
        let mut output = Vec::new();
        let exit = second_writer.write_response(&mut output, cancelled_response());
        done.send((exit, output)).unwrap();
    });
    assert!(completed.recv_timeout(SHORT_WAIT).is_err());
    resume.send(()).unwrap();
    let first_exit = first.join().unwrap();
    let (second_exit, second_output) = completed.recv_timeout(WAIT).unwrap();
    second.join().unwrap();
    // Then: flush completes before the loser observes the winning exit code.
    assert_eq!(first_exit, 0);
    assert_eq!(&*bytes.lock().unwrap(), SUCCESS_ENVELOPE);
    assert_eq!(second_exit, 0);
    assert!(second_output.is_empty());
}

#[test]
fn cancel_output_wins_before_main_output() {
    // Given: the command thread pauses before it attempts any output.
    let writer = Arc::new(ResponseWriter::default());
    let (entered, main_entered) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let main_writer = Arc::clone(&writer);
    let main = thread::spawn(move || {
        entered.send(()).unwrap();
        released.recv().unwrap();
        let mut output = Vec::new();
        let exit = main_writer.write_response(&mut output, successful_response());
        (exit, output)
    });
    main_entered.recv_timeout(WAIT).unwrap();
    // When: cancellation completes and flushes before main is released.
    let mut output = Vec::new();
    let cancellation_exit = writer.write_response(&mut output, cancelled_response());
    release.send(()).unwrap();
    let (normal_exit, normal_output) = main.join().unwrap();
    // Then: main receives the winning exit code and appends no second JSON.
    assert_eq!(cancellation_exit, 1);
    assert_eq!(output, CANCELLED_ENVELOPE);
    assert_eq!(normal_exit, 1);
    assert!(normal_output.is_empty());
}

#[test]
fn repeated_cancellation_responses_keep_one_envelope() {
    // Given: the first cancellation response owns the output.
    let writer = ResponseWriter::default();
    let mut output = Vec::new();
    let first_exit = writer.write_response(&mut output, cancelled_response());
    // When: later callers offer the same cancellation response again.
    for _ in 0..2 {
        let mut repeated = Vec::new();
        let exit = writer.write_response(&mut repeated, cancelled_response());
        assert_eq!(exit, 1);
        assert!(repeated.is_empty());
    }
    // Then: exactly one complete cancellation envelope exists.
    assert_eq!(first_exit, 1);
    assert_eq!(output, CANCELLED_ENVELOPE);
    assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
}

fn successful_response() -> CommandResult<CommandResponse> {
    Ok(CommandResponse {
        result: ResponseResult::Drained,
        warnings: Vec::new(),
    })
}

fn cancelled_response() -> CommandResult<CommandResponse> {
    Err(CommandError::new(
        CommandErrorKind::Cancelled,
        "command cancelled",
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pause {
    Write,
    Flush,
}

struct PausingWriter {
    pause: Pause,
    bytes: Arc<Mutex<Vec<u8>>>,
    entered: Sender<Pause>,
    resume: Receiver<()>,
}

struct PausedOutput {
    writer: PausingWriter,
    bytes: Arc<Mutex<Vec<u8>>>,
    entered: Receiver<Pause>,
    resume: Sender<()>,
}

impl PausedOutput {
    fn new(pause: Pause) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (entered, entered_receiver) = mpsc::channel();
        let (resume, resume_receiver) = mpsc::channel();
        let writer = PausingWriter {
            pause,
            bytes: Arc::clone(&bytes),
            entered,
            resume: resume_receiver,
        };
        Self {
            writer,
            bytes,
            entered: entered_receiver,
            resume,
        }
    }
}

impl Write for PausingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(buffer);
        if self.pause == Pause::Write {
            self.pause_here();
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.pause == Pause::Flush {
            self.pause_here();
        }
        Ok(())
    }
}

impl PausingWriter {
    fn pause_here(&mut self) {
        self.entered.send(self.pause).unwrap();
        self.resume.recv().unwrap();
    }
}

struct FailingWriter {
    attempts: Arc<Mutex<usize>>,
}

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        *self.attempts.lock().unwrap() += 1;
        Err(io::Error::other("write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
