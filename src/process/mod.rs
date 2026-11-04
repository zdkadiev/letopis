//! Detached background launch and foreground signal lifecycle.
mod detached;
mod input;
mod signal;
pub(crate) use detached::DetachedProcess;
pub(crate) use input::ProcessInput;
/// Owns foreground SIGINT registration, listener teardown and handled exit.
pub(crate) use signal::SigintListener;

mod clock;
pub(crate) use clock::SystemClock;
