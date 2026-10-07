//! A cancellation request belongs to one foreground core invocation.
//!
//! Manual and catalog setup commands can opt in. Their parent keeps stdin's
//! pipe writer open while the invocation runs and closes it to request
//! cancellation. This never signals a saved PID or another bottle's Wine server.

use std::cell::RefCell;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

pub(super) const ERROR: &str = "Setup canceled";
pub(super) const EXIT_CODE: i32 = 130;

const RUNNING: u8 = 0;
const CANCELED: u8 = 1;
const TIMEOUT_CLEANUP: u8 = 2;
const CLEANUP_CANCEL_PENDING: u8 = 3;

struct RequestState {
    phase: AtomicU8,
}

impl RequestState {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(RUNNING),
        }
    }

    fn request_cancel(&self) {
        loop {
            let current = self.phase.load(Ordering::Acquire);
            let next = match current {
                RUNNING => CANCELED,
                TIMEOUT_CLEANUP => CLEANUP_CANCEL_PENDING,
                CANCELED | CLEANUP_CANCEL_PENDING => return,
                _ => unreachable!("invalid cancellation phase"),
            };
            if self
                .phase
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return;
            }
        }
    }
}

thread_local! {
    static REQUEST: RefCell<Option<Arc<RequestState>>> = const { RefCell::new(None) };
}

pub(super) fn activate_from_stdin() {
    if std::env::var("CASKRUN_CANCEL_STDIN").ok().as_deref() != Some("1") {
        return;
    }
    activate_reader(std::io::stdin());
}

fn activate_reader(mut reader: impl Read + Send + 'static) {
    let request = Arc::new(RequestState::new());
    REQUEST.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&request)));
    std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        loop {
            match reader.read(&mut byte) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                // EOF is the normal request. Any unexpected input or read error
                // also fails closed instead of leaving an installer unattended.
                _ => {
                    request.request_cancel();
                    break;
                }
            }
        }
    });
}

pub(super) fn is_requested() -> bool {
    REQUEST.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|request| request.phase.load(Ordering::Acquire) == CANCELED)
    })
}

/// Atomically claim a timeout cleanup before issuing a bottle-wide wineserver
/// request. Cancellation that won first prevents cleanup. Cancellation during
/// cleanup is deferred until the returned guard is dropped.
pub(super) fn begin_timeout_cleanup() -> Result<TimeoutCleanupGuard, String> {
    let request = REQUEST.with(|slot| slot.borrow().clone());
    let Some(request) = request else {
        return Ok(TimeoutCleanupGuard(None));
    };
    loop {
        match request.phase.load(Ordering::Acquire) {
            RUNNING => {
                if request
                    .phase
                    .compare_exchange(
                        RUNNING,
                        TIMEOUT_CLEANUP,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return Ok(TimeoutCleanupGuard(Some(request)));
                }
            }
            CANCELED => return Err(ERROR.into()),
            TIMEOUT_CLEANUP | CLEANUP_CANCEL_PENDING => {
                return Err("Timeout cleanup is already in progress".into());
            }
            _ => unreachable!("invalid cancellation phase"),
        }
    }
}

pub(super) struct TimeoutCleanupGuard(Option<Arc<RequestState>>);

impl Drop for TimeoutCleanupGuard {
    fn drop(&mut self) {
        let Some(request) = self.0.as_ref() else {
            return;
        };
        loop {
            let current = request.phase.load(Ordering::Acquire);
            let next = match current {
                TIMEOUT_CLEANUP => RUNNING,
                CLEANUP_CANCEL_PENDING => CANCELED,
                _ => return,
            };
            if request
                .phase
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return;
            }
        }
    }
}

pub(super) fn check() -> Result<(), String> {
    if is_requested() {
        Err(ERROR.into())
    } else {
        Ok(())
    }
}

pub(super) fn is_error(error: &str) -> bool {
    error.starts_with(ERROR)
}

pub(super) fn is_active() -> bool {
    REQUEST.with(|slot| slot.borrow().is_some())
}

#[cfg(test)]
pub(super) struct TestRequestGuard;

#[cfg(test)]
impl Drop for TestRequestGuard {
    fn drop(&mut self) {
        REQUEST.with(|slot| *slot.borrow_mut() = None);
    }
}

#[cfg(test)]
pub(super) struct TestRequest(Arc<RequestState>);

#[cfg(test)]
impl TestRequest {
    pub(super) fn store(&self, value: bool, _ordering: Ordering) {
        assert!(value, "test cancellation requests are monotonic");
        self.0.request_cancel();
    }
}

#[cfg(test)]
pub(super) fn test_request() -> (TestRequest, TestRequestGuard) {
    let request = Arc::new(RequestState::new());
    REQUEST.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&request)));
    (TestRequest(request), TestRequestGuard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    #[test]
    fn closing_one_invocations_writer_requests_only_that_invocation() {
        let (reader, writer) = UnixStream::pair().unwrap();
        activate_reader(reader);
        assert!(!is_requested());
        drop(writer);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !is_requested() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(check().unwrap_err(), ERROR);
        REQUEST.with(|slot| *slot.borrow_mut() = None);
        assert!(check().is_ok());
    }

    #[test]
    fn cancel_before_timeout_cleanup_prevents_the_claim() {
        let (request, _guard) = test_request();
        request.store(true, Ordering::Release);
        assert_eq!(begin_timeout_cleanup().err().unwrap(), ERROR);
        assert!(is_requested());
    }

    #[test]
    fn timeout_cleanup_without_opt_in_preserves_the_existing_path() {
        assert!(!is_active());
        let cleanup = begin_timeout_cleanup().unwrap();
        assert!(check().is_ok());
        drop(cleanup);
        assert!(check().is_ok());
    }

    #[test]
    fn cancel_during_timeout_cleanup_is_deferred_until_guard_drops() {
        let (request, _guard) = test_request();
        let cleanup = begin_timeout_cleanup().unwrap();
        request.store(true, Ordering::Release);
        assert!(!is_requested());
        assert!(check().is_ok());
        drop(cleanup);
        assert!(is_requested());
        assert_eq!(check().unwrap_err(), ERROR);
    }

    #[test]
    fn concurrent_cancel_and_timeout_claim_never_lose_the_cancel() {
        for _ in 0..100 {
            let (request, guard) = test_request();
            let barrier = Arc::new(Barrier::new(2));
            let other_barrier = Arc::clone(&barrier);
            let cancel_thread = std::thread::spawn(move || {
                other_barrier.wait();
                request.store(true, Ordering::Release);
            });
            barrier.wait();
            match begin_timeout_cleanup() {
                Ok(cleanup) => {
                    cancel_thread.join().unwrap();
                    assert!(!is_requested());
                    drop(cleanup);
                    assert!(is_requested());
                }
                Err(error) => {
                    cancel_thread.join().unwrap();
                    assert_eq!(error, ERROR);
                    assert!(is_requested());
                }
            }
            drop(guard);
        }
    }
}
