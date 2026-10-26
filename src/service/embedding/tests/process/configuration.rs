use super::{
    Fixture, LIMIT, Process, SUCCESS, assert_current_vector, open_storage, read_http_request,
    reply, reset_with_current_state,
};
use crate::{
    service::{Embedder, Storage, VectorOutcome, VectorWrite},
    wire_embedder,
};
use std::{
    io::Write,
    thread,
    time::{Duration, Instant},
};

#[test]
fn stale_configuration_observation_preserves_current_vectors_and_attempts() {
    // Given
    for action in ["drain", "query", "readiness", "configuration"] {
        // When / Then: the helper runs and checks the process scenario.
        assert_stale_observation(action);
    }
}

fn assert_stale_observation(action: &str) {
    // Given: P validates A while its original storage observation is still current.
    let fixture = Fixture::new(0);
    let mut service = wire_embedder(fixture.root.path(), open_storage(&fixture.root)).unwrap();
    service.read_configuration().unwrap();
    drop(service);
    let mut child = Process::spawn(
        &fixture,
        "stale-config",
        "configuration",
        action,
        "ConfigChanged",
    );
    let mut barrier = child.await_checkpoint();
    let next = reset_with_current_state(&fixture);
    let mut storage = open_storage(&fixture.root);
    let VectorOutcome::Current(mut unit) =
        storage.begin_vector_write(&next.get_identity()).unwrap()
    else {
        panic!()
    };
    assert!(unit.reserve_attempt(&fixture.hash, 4).unwrap());
    unit.commit().unwrap();
    let before = fixture.capture_cache();
    // When: obsolete P resumes after B has stored both successful and pending work.
    barrier.write_all(b"C").unwrap();
    let requests = finish_http_requests(&fixture, &mut child);
    // Then: check stored state first; baseline must fail on destroyed B state.
    fixture.assert_state(&next, None, Some(1));
    assert_current_vector(&fixture, &next);
    assert_eq!(fixture.capture_cache(), before);
    assert_eq!(requests, 0, "obsolete configuration must never reach HTTP");
    child.finish();
}

fn finish_http_requests(fixture: &Fixture, child: &mut Process) -> usize {
    let mut requests = 0;
    let start = Instant::now();
    while child.child.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < LIMIT, "child watchdog timeout");
        if let Ok((stream, _)) = fixture.listener.accept() {
            requests += 1;
            reply(read_http_request(stream), SUCCESS);
        }
        thread::sleep(Duration::from_millis(5));
    }
    while let Ok((stream, _)) = fixture.listener.accept() {
        requests += 1;
        reply(read_http_request(stream), SUCCESS);
    }
    requests
}
