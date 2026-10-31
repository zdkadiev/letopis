#[path = "support/parallel_drain.rs"]
mod drain;
mod support;

use drain::{assert_success, prepare_repo, read_hash, read_reserved, read_vector, spawn_drain};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(3);

#[test]
fn drains_http10_responses_without_reusing_implicitly_closed_connections() {
    // Given: valid length-delimited HTTP/1.0 responses without an explicit close header.
    let repo = prepare_repo();
    let provider = Http10Provider::start();
    let config = repo.write_config("ollama", &provider.url, "fixture");
    // When: one real drain embeds eight texts through the shared concurrent client.
    let child = spawn_drain(&repo);
    provider.wait_requests(8);
    assert_success(&child.wait(Instant::now() + WAIT));
    // Then: every text has its literal vector; none spent its only attempt on socket reuse.
    assert_eq!(read_reserved(&repo), 0);
    for number in 0..8 {
        let hash = read_hash(&repo, &format!("k{number}.md"));
        assert_eq!(read_vector(&repo, &config, &hash), Some(vec![1., 2., 3.]));
    }
}

#[derive(Debug, Default)]
struct State {
    requests: usize,
    stopped: bool,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Debug)]
struct Http10Provider {
    url: String,
    shared: Arc<Shared>,
    server: Option<JoinHandle<()>>,
}

impl Http10Provider {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/api", listener.local_addr().unwrap());
        let shared = Arc::new(Shared::default());
        let server_state = Arc::clone(&shared);
        let server = Some(thread::spawn(move || serve(&listener, &server_state)));
        Self {
            url,
            shared,
            server,
        }
    }

    fn wait_requests(&self, count: usize) {
        let state = self.shared.state.lock().unwrap();
        let (state, _) = self
            .shared
            .wake
            .wait_timeout_while(state, WAIT, |state| state.requests < count)
            .unwrap();
        let observed = state.requests;
        drop(state);
        assert_eq!(
            observed, count,
            "HTTP/1.0 requests must use independent connections"
        );
    }
}

impl Drop for Http10Provider {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().stopped = true;
        self.shared.wake.notify_all();
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
    }
}

fn serve(listener: &TcpListener, shared: &Arc<Shared>) {
    let mut handlers = Vec::new();
    while !shared.state.lock().unwrap().stopped {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = Arc::clone(shared);
                handlers.push(thread::spawn(move || respond(stream, &state)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("owned HTTP/1.0 listener failed: {error}"),
        }
    }
    for handler in handlers {
        handler.join().unwrap();
    }
}

fn respond(mut stream: TcpStream, shared: &Shared) {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    stream.set_write_timeout(Some(WAIT)).unwrap();
    let request = read_request(&stream);
    assert_eq!(request["model"], "fixture");
    assert_eq!(request["input"].as_array().unwrap().len(), 1);
    let body = r#"{"model":"fixture","embeddings":[[1,2,3]]}"#;
    write!(
        stream,
        "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    stream.flush().unwrap();
    let mut state = shared.state.lock().unwrap();
    state.requests += 1;
    shared.wake.notify_all();
    // Keep the socket open after the complete body: HTTP/1.0 still forbids reuse
    // without keep-alive. A gate makes that race deterministic rather than timed.
    state = shared
        .wake
        .wait_while(state, |state| !state.stopped)
        .unwrap();
    drop(state);
}

fn read_request(stream: &TcpStream) -> Value {
    let mut reader = BufReader::new(stream);
    let mut length = None;
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut body = vec![0; length.unwrap()];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}
