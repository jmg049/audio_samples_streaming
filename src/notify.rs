use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Wakeup primitive for crossing the RT/non-RT boundary.
///
/// The real-time cpal callback calls [`notify`](Self::notify) — which only calls
/// `Condvar::notify_one()`, never acquires the mutex, and is therefore safe on
/// a real-time thread.
///
/// The non-RT side calls [`wait`](Self::wait), which blocks on the condvar with a
/// short fallback timeout so missed notifications do not cause permanent stalls.
pub(crate) struct AudioNotifier {
    condvar: Condvar,
    mutex: Mutex<()>,
}

impl AudioNotifier {
    pub(crate) fn new() -> Self {
        Self {
            condvar: Condvar::new(),
            mutex: Mutex::new(()),
        }
    }

    /// Called from the cpal RT callback. Never acquires the mutex.
    #[inline]
    pub(crate) fn notify(&self) {
        self.condvar.notify_one();
    }

    /// Block until notified or the fallback timeout elapses.
    ///
    /// The 500 µs fallback guards against the race where `notify` fires between
    /// the caller's slots check and entering `wait`. In normal operation the
    /// condvar wakes us up immediately — the fallback is a safety net, not the
    /// common path.
    #[inline]
    pub(crate) fn wait(&self) {
        let guard = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.condvar.wait_timeout(guard, Duration::from_micros(500));
    }
}
