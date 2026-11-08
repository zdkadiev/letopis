use signal_hook::{
    consts::SIGINT,
    iterator::{Handle, Signals},
};
use std::{
    io, process,
    thread::{self, JoinHandle},
};

const LISTENER_THREAD_NAME: &str = "letopis-sigint";

/// Owns the process SIGINT subscription for one executable invocation.
///
/// The listener invokes its callback once when SIGINT arrives and then exits
/// the process with status 1. Normal teardown closes the subscription so the
/// blocking iterator wakes, and joins the listener thread.
#[derive(Debug)]
pub(crate) struct SigintListener {
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl SigintListener {
    /// Registers SIGINT and starts the dedicated listener thread.
    ///
    /// # Arguments
    ///
    /// * `on_interrupt` — the callback invoked once when SIGINT arrives, immediately before
    ///   the process exits with status 1.
    ///
    /// # Errors
    /// Returns an I/O error when signal registration or thread creation fails.
    /// No partially started listener is retained.
    pub(crate) fn signal_listener_install(
        on_interrupt: Box<dyn FnOnce() + Send>,
    ) -> io::Result<Self> {
        let signals = Signals::new([SIGINT])?;
        Self::signal_listener_start_with(signals, signal_listener_spawn, on_interrupt)
    }

    #[cfg(test)]
    fn signal_listener_install_with(
        register: impl FnOnce() -> io::Result<Signals>,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send + 'static>) -> io::Result<JoinHandle<()>>,
        on_interrupt: Box<dyn FnOnce() + Send>,
    ) -> io::Result<Self> {
        Self::signal_listener_start_with(register()?, spawn, on_interrupt)
    }

    fn signal_listener_start_with(
        mut signals: Signals,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send + 'static>) -> io::Result<JoinHandle<()>>,
        on_interrupt: Box<dyn FnOnce() + Send>,
    ) -> io::Result<Self> {
        let handle = signals.handle();
        let listener = Box::new(move || {
            if signals.forever().next().is_some() {
                on_interrupt();
                process::exit(1);
            }
        });
        let thread = spawn(listener)?;
        Ok(Self {
            handle,
            thread: Some(thread),
        })
    }
}

impl Drop for SigintListener {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn signal_listener_spawn(
    listener: Box<dyn FnOnce() + Send + 'static>,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(LISTENER_THREAD_NAME.to_owned())
        .spawn(listener)
}

#[cfg(test)]
#[path = "signal_test.rs"]
mod tests;
