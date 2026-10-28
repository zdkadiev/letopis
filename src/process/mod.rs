//! Detached background launch and foreground signal lifecycle.
mod detached;
mod signal;
pub(crate) use detached::DetachedProcess;
/// Owns foreground SIGINT registration, listener teardown and handled exit.
pub(crate) use signal::SigintListener;
