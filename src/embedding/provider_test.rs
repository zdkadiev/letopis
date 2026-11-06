use super::{Http, HttpProviders, build_authorization, classify};
use crate::service::EmbeddingErrorKind;
use serde_json::json;
use std::{
    io::{self, ErrorKind, Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

#[test]
fn redacts_captured_credentials_from_owner_debug() {
    // Given: the real provider owner captures a synthetic compatible credential.
    let provider = HttpProviders::new(Some("owner-debug-secret".to_owned()));
    // When: both ordinary and pretty Debug formatting observe the owner.
    let ordinary = format!("{provider:?}");
    let pretty = format!("{provider:#?}");
    // Then: the literal representation contains no credential or transport internals.
    assert_eq!(ordinary, "HttpProviders { .. }");
    assert_eq!(pretty, "HttpProviders { .. }");
}

#[test]
fn accepts_only_nonempty_printable_ascii_credentials() {
    // Given: boundary characters are valid, while absent, empty and nonprintable keys are not.
    for (key, expected) in [
        (Some("!fixture~"), Some("Bearer !fixture~")),
        (None, None),
        (Some(""), None),
        (Some("fixture secret"), None),
        (Some("fixture\nsecret"), None),
        (Some("fixture\u{7f}secret"), None),
        (Some("fixture\u{80}secret"), None),
    ] {
        // When: compatible authorization is prepared without sending a request.
        let result = build_authorization(key);
        // Then: only the independently specified printable key produces a header.
        match expected {
            Some(header) => assert_eq!(result.unwrap(), header),
            None => {
                let error = result.unwrap_err();
                assert_eq!(error.get_kind(), EmbeddingErrorKind::Credentials);
                assert!(!format!("{error} {error:?}").contains("secret"));
            }
        }
    }
}

#[test]
fn bounds_waiting_for_response_headers() {
    // Given / When / Then: the helper configures, calls, and verifies the deadline.
    run_deadline_case(Duration::from_millis(240), Duration::ZERO);
}

#[test]
fn bounds_waiting_for_response_body() {
    // Given / When / Then: the helper configures, calls, and verifies the deadline.
    run_deadline_case(Duration::ZERO, Duration::from_millis(240));
}

#[test]
fn shares_one_deadline_across_headers_and_body() {
    // Given / When / Then: the helper configures, calls, and verifies the deadline.
    run_deadline_case(Duration::from_millis(80), Duration::from_millis(80));
}

#[test]
fn classifies_io_timeouts_without_exposing_transport_payloads() {
    // Given
    for (io_kind, expected) in [
        (ErrorKind::TimedOut, EmbeddingErrorKind::Timeout),
        (ErrorKind::ConnectionReset, EmbeddingErrorKind::Connection),
    ] {
        let cause = ureq::Error::Io(io::Error::new(io_kind, "transport-secret"));
        // When
        let error = classify(cause);
        // Then
        assert_eq!(error.get_kind(), expected);
        assert!(!format!("{error} {error:?}").contains("transport-secret"));
    }
}

fn run_deadline_case(header_delay: Duration, body_delay: Duration) {
    // Given: one local response delays its headers, body or both beyond a short shared deadline.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/embed", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut received = Vec::new();
        loop {
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            received.push(byte[0]);
            if received.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        thread::sleep(header_delay);
        let _ =
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n");
        thread::sleep(body_delay);
        let _ = socket.write_all(b"{}");
    });
    let http = Http::new_with_deadline(Duration::from_millis(120));
    let start = Instant::now();
    // When: the real HTTP transport sends one request and waits for its response.
    let result = http.request_send_by_endpoint(&endpoint, &json!({"input":["fixture"]}), None);
    let elapsed = start.elapsed();
    worker.join().unwrap();
    // Then: the request times out within the independent two-second outer bound.
    assert_eq!(result.unwrap_err().get_kind(), EmbeddingErrorKind::Timeout);
    assert!(elapsed < Duration::from_secs(2));
}
