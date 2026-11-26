//! Live CuraEngine processes.
//!
//! A slice owns its child. Cancel, and closing the window, both have to stop
//! that child even when the wait loop is on another thread. The registry is
//! the list of children that are still running. Dropping a tracked child, or
//! calling [`cancel_running_engines`], kills it and reaps it.

use std::io;
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

struct LiveEngine {
    child: Mutex<Option<Child>>,
    cancelled: AtomicBool,
}

fn registry() -> &'static Mutex<Vec<Arc<LiveEngine>>> {
    static REGISTRY: OnceLock<Mutex<Vec<Arc<LiveEngine>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock_registry() -> std::sync::MutexGuard<'static, Vec<Arc<LiveEngine>>> {
    match registry().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Stop every CuraEngine this process started and has not reaped yet.
pub fn cancel_running_engines() {
    let engines = {
        let mut guard = lock_registry();
        std::mem::take(&mut *guard)
    };
    for engine in engines {
        engine.cancelled.store(true, Ordering::SeqCst);
        shutdown(&engine);
    }
}

fn shutdown(engine: &LiveEngine) {
    let mut slot = match engine.child.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(mut child) = slot.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// What [`TrackedChild::poll`] found.
pub enum ChildPoll {
    Running,
    Finished(ExitStatus),
    Cancelled,
}

/// One engine process. Dropping it kills the process if it is still running.
pub struct TrackedChild {
    engine: Arc<LiveEngine>,
}

impl TrackedChild {
    pub fn adopt(child: Child) -> Self {
        let engine = Arc::new(LiveEngine {
            child: Mutex::new(Some(child)),
            cancelled: AtomicBool::new(false),
        });
        lock_registry().push(Arc::clone(&engine));
        Self { engine }
    }

    pub fn take_pipes(
        &self,
    ) -> (
        Option<std::process::ChildStdout>,
        Option<std::process::ChildStderr>,
    ) {
        let mut slot = match self.engine.child.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match slot.as_mut() {
            Some(child) => (child.stdout.take(), child.stderr.take()),
            None => (None, None),
        }
    }

    pub fn poll(&self) -> io::Result<ChildPoll> {
        if self.engine.cancelled.load(Ordering::SeqCst) {
            shutdown(&self.engine);
            return Ok(ChildPoll::Cancelled);
        }
        let mut slot = match self.engine.child.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match slot.as_mut() {
            Some(child) => match child.try_wait()? {
                Some(status) => Ok(ChildPoll::Finished(status)),
                None => Ok(ChildPoll::Running),
            },
            None => Ok(ChildPoll::Cancelled),
        }
    }
}

impl Drop for TrackedChild {
    fn drop(&mut self) {
        {
            let mut guard = lock_registry();
            guard.retain(|item| !Arc::ptr_eq(item, &self.engine));
        }
        shutdown(&self.engine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn linger() -> Command {
        if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "40", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        }
    }

    #[test]
    fn cancelling_stops_the_engine_process() {
        let child = linger().spawn().expect("start a stand-in for CuraEngine");
        let tracked = TrackedChild::adopt(child);
        let started = Instant::now();
        cancel_running_engines();
        let mut stopped = false;
        for _ in 0..50 {
            match tracked.poll().expect("poll") {
                ChildPoll::Cancelled | ChildPoll::Finished(_) => {
                    stopped = true;
                    break;
                }
                ChildPoll::Running => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(stopped, "the process was still running");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
