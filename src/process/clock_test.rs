use super::SystemClock;
use crate::service::Clock;
use std::time::Duration;

#[test]
fn observes_epoch_time_and_accepts_zero_wait() {
    // Given: the synchronous system clock used by application services.
    let clock = SystemClock::new();
    // When: both supported units and a zero-duration retry wait are requested.
    let nanoseconds = clock.clock_get_time_ns().unwrap();
    let seconds = clock.clock_get_time_secs().unwrap();
    clock.clock_wait(Duration::ZERO).unwrap();
    // Then: both observations use the positive Unix epoch, without overflow.
    assert!(nanoseconds > 0);
    assert!(seconds > 0);
}
