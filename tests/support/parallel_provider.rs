use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(3);

/// One synthetic request independently observed at the HTTP boundary.
#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub index: usize,
    pub text: String,
}

#[derive(Clone, Debug)]
struct Reply {
    status: u16,
    body: String,
}

#[derive(Debug, Default)]
struct State {
    requests: Vec<CapturedRequest>,
    replies: BTreeMap<usize, Reply>,
    fallback: Option<Reply>,
    active: usize,
    maximum: usize,
    stopped: bool,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Owns a concurrent loopback provider and every handler it starts.
#[derive(Debug)]
pub struct ParallelProvider {
    url: String,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl ParallelProvider {
    /// Bind loopback and hold each response until explicitly released.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let shared = Arc::new(Shared::default());
        let owner = Arc::clone(&shared);
        let worker = thread::spawn(move || serve(&listener, &owner));
        Self {
            url,
            shared,
            worker: Some(worker),
        }
    }

    pub fn get_base_url(&self) -> &str {
        &self.url
    }

    /// Wait for an independently counted number of actual HTTP requests.
    pub fn wait_requests(&self, count: usize) -> Vec<CapturedRequest> {
        let state = self.shared.state.lock().unwrap();
        let (state, _) = self
            .shared
            .wake
            .wait_timeout_while(state, WAIT, |state| state.requests.len() < count)
            .unwrap();
        let requests = state.requests.clone();
        drop(state);
        assert!(
            requests.len() >= count,
            "expected {count} concurrent requests; observed {}",
            requests.len()
        );
        requests
    }

    /// Wait until every currently held response has been released for transmission.
    pub fn wait_idle(&self) {
        let state = self.shared.state.lock().unwrap();
        let (state, _) = self
            .shared
            .wake
            .wait_timeout_while(state, WAIT, |state| state.active != 0)
            .unwrap();
        let active = state.active;
        drop(state);
        assert_eq!(active, 0, "provider responses remain held");
    }

    pub fn get_requests(&self) -> Vec<CapturedRequest> {
        self.shared.state.lock().unwrap().requests.clone()
    }

    pub fn get_maximum_active(&self) -> usize {
        self.shared.state.lock().unwrap().maximum
    }

    pub fn get_active(&self) -> usize {
        self.shared.state.lock().unwrap().active
    }

    /// Release just the addressed request with an independently chosen vector.
    pub fn release(&self, index: usize, vector: &[f32]) {
        self.release_response(
            index,
            200,
            json!({"model":"fixture", "embeddings":[vector]}).to_string(),
        );
    }

    pub fn release_response(&self, index: usize, status: u16, body: String) {
        self.shared
            .state
            .lock()
            .unwrap()
            .replies
            .insert(index, Reply { status, body });
        self.shared.wake.notify_all();
    }

    /// Release present and future requests, including during assertion cleanup.
    pub fn release_all(&self) {
        self.shared.state.lock().unwrap().fallback = Some(Reply {
            status: 200,
            body: json!({"model":"fixture", "embeddings":[[1.,2.,3.]]}).to_string(),
        });
        self.shared.wake.notify_all();
    }
}

impl Drop for ParallelProvider {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().stopped = true;
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn serve(listener: &TcpListener, shared: &Arc<Shared>) {
    let mut handlers = Vec::new();
    while !shared.state.lock().unwrap().stopped {
        match listener.accept() {
            Ok((stream, _)) => {
                let owner = Arc::clone(shared);
                handlers.push(thread::spawn(move || handle_request(stream, &owner)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    }
    for handler in handlers {
        handler.join().unwrap();
    }
}

fn handle_request(mut stream: TcpStream, shared: &Shared) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    let text = request["input"][0].as_str().unwrap().to_owned();
    assert_eq!(request["input"].as_array().unwrap().len(), 1);
    let mut state = shared.state.lock().unwrap();
    let index = state.requests.len();
    state.requests.push(CapturedRequest { index, text });
    state.active += 1;
    state.maximum = state.maximum.max(state.active);
    shared.wake.notify_all();
    let reply = wait_reply(shared, state, index);
    shared.state.lock().unwrap().active -= 1;
    shared.wake.notify_all();
    if let Some(reply) = reply {
        let header = format!(
            "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reply.status,
            reply.body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(reply.body.as_bytes());
    }
}

fn wait_reply(
    shared: &Shared,
    mut state: std::sync::MutexGuard<'_, State>,
    index: usize,
) -> Option<Reply> {
    loop {
        if state.stopped {
            return None;
        }
        if let Some(reply) = state
            .replies
            .remove(&index)
            .or_else(|| state.fallback.clone())
        {
            return Some(reply);
        }
        state = shared.wake.wait(state).unwrap();
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Value> {
    // macOS can inherit the listener's nonblocking flag on accepted sockets.
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        if let Err(error) = stream.read_exact(&mut byte) {
            // A connection without an HTTP request consumes no fixture job.
            assert!(headers.is_empty(), "incomplete fixture request: {error}");
            return None;
        }
        headers.push(byte[0]);
        assert!(headers.len() < 16384);
    }
    let headers = String::from_utf8(headers).unwrap();
    assert!(headers.starts_with("POST /api/embed HTTP/1.1\r\n"));
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    assert!(length < 1024 * 1024);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    Some(serde_json::from_slice(&body).unwrap())
}
