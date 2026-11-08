use crate::service::Clock;
use std::{
    io,
    time::{Duration, SystemTime},
};

/// Synchronous process wall clock and thread wait.
#[derive(Debug, Default)]
pub(crate) struct SystemClock {
    #[cfg(test)]
    time: Option<i64>,
}

#[cfg(test)]
thread_local! { static NEXT_TIME: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) }; }

impl SystemClock {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(test)]
            time: NEXT_TIME.take(),
        }
    }

    #[cfg(test)]
    pub(crate) fn at(time: i64) -> Self {
        Self { time: Some(time) }
    }

    #[cfg(test)]
    pub(crate) fn configure_next(time: i64) {
        NEXT_TIME.set(Some(time));
    }
}

impl Clock for SystemClock {
    fn clock_get_time_ns(&self) -> io::Result<i64> {
        #[cfg(test)]
        if let Some(time) = self.time {
            return Ok(time);
        }
        let elapsed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(io::Error::other)?;
        i64::try_from(elapsed.as_nanos()).map_err(io::Error::other)
    }

    fn clock_get_time_secs(&self) -> io::Result<u64> {
        Ok(SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs())
    }

    fn clock_wait(&self, delay: Duration) -> io::Result<()> {
        std::thread::sleep(delay);
        Ok(())
    }
}

#[cfg(test)]
#[path = "clock_test.rs"]
mod tests;
