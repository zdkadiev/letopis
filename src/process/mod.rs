//! Detached background launch and foreground signal lifecycle.
mod detached;
mod signal;
pub(crate) use detached::DetachedProcess;
pub(crate) use signal::SigintListener;
