use serde_json::Value;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone)]
pub enum FixtureReply {
    Status {
        status: u16,
        body: String,
    },
    CloseDelimited {
        body: String,
    },
    Redirect {
        location: String,
    },
    DelayedHeaders {
        delay: Duration,
        body: String,
    },
    DelayedBody {
        delay: Duration,
        body: String,
    },
    Barrier {
        barrier: ResponseBarrier,
        body: String,
    },
}
#[derive(Clone, Default)]
pub struct ResponseBarrier(Arc<(Mutex<(bool, bool)>, Condvar)>);
impl ResponseBarrier {
    pub fn wait_until_entered(&self) {
        let (lock, wake) = &*self.0;
        let state = lock.lock().unwrap();
        let (state, timeout) = wake
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.0)
            .unwrap();
        assert!(
            state.0 && !timeout.timed_out(),
            "provider did not reach barrier"
        );
    }
    pub fn release(&self) {
        let (lock, wake) = &*self.0;
        lock.lock().unwrap().1 = true;
        wake.notify_all();
    }
    fn wait(&self, stop: &AtomicBool) {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().unwrap();
        state.0 = true;
        wake.notify_all();
        while !state.1 && !stop.load(Ordering::Relaxed) {
            state = wake
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap()
                .0;
        }
    }
}
#[derive(Default)]
struct Captures {
    requests: Vec<Value>,
    headers: Vec<String>,
}
pub struct HttpFixture {
    url: String,
    stop: Arc<AtomicBool>,
    captures: Arc<Mutex<Captures>>,
    worker: Option<JoinHandle<()>>,
}
impl HttpFixture {
    pub fn start(reply: FixtureReply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let captures = Arc::new(Mutex::new(Captures::default()));
        let worker_stop = Arc::clone(&stop);
        let worker_captures = Arc::clone(&captures);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        capture_request(&mut stream, &worker_captures);
                        write_reply(&mut stream, &reply, &worker_stop);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
        });
        Self {
            url: format!("http://{address}/api"),
            stop,
            captures,
            worker: Some(worker),
        }
    }
    pub fn get_base_url(&self) -> String {
        self.url.clone()
    }
    pub fn take_requests(&self) -> Vec<Value> {
        std::mem::take(&mut self.captures.lock().unwrap().requests)
    }
    pub fn take_headers(&self) -> Vec<String> {
        std::mem::take(&mut self.captures.lock().unwrap().headers)
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}
fn capture_request(stream: &mut TcpStream, captures: &Mutex<Captures>) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let headers = String::from_utf8(bytes.clone()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    bytes.resize(end + length, 0);
    stream.read_exact(&mut bytes[end..]).unwrap();
    let mut captures = captures.lock().unwrap();
    captures
        .requests
        .push(serde_json::from_slice(&bytes[end..]).unwrap());
    captures.headers.push(headers);
}
fn pause(delay: Duration, stop: &AtomicBool) {
    let start = std::time::Instant::now();
    while start.elapsed() < delay && !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(2));
    }
}
fn write_reply(stream: &mut TcpStream, reply: &FixtureReply, stop: &AtomicBool) {
    let (status, body, extra) = match reply {
        FixtureReply::Status { status, body } => (*status, body.as_str(), String::new()),
        FixtureReply::Redirect { location } => (302, "", format!("Location: {location}\r\n")),
        FixtureReply::DelayedHeaders { delay, body } => {
            pause(*delay, stop);
            (200, body.as_str(), String::new())
        }
        FixtureReply::CloseDelimited { body } | FixtureReply::DelayedBody { body, .. } => {
            (200, body.as_str(), String::new())
        }
        FixtureReply::Barrier { barrier, body } => {
            barrier.wait(stop);
            (200, body.as_str(), String::new())
        }
    };
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let length = if matches!(reply, FixtureReply::CloseDelimited { .. }) {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    let header = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\n{length}Connection: close\r\n{extra}\r\n"
    );
    if stream.write_all(header.as_bytes()).is_err() {
        return;
    }
    if let FixtureReply::DelayedBody { delay, .. } = reply {
        pause(*delay, stop);
    }
    let _ = stream.write_all(body.as_bytes());
}
