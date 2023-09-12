//! This module provides a Lazy type that is used internally for initializing
//! the LoggerHandle singleton.

#[cfg(not(all(test, feature = "loom")))]
use core::sync::atomic::{Ordering, AtomicU8};
#[cfg(not(all(test, feature = "loom")))]
use tokio::task::yield_now;

#[cfg(all(test, feature = "loom"))]
use loom::{
    thread::yield_now,
    sync::atomic::{Ordering, AtomicU8}
};

use tokio::time::Duration;

use crate::error::LoggerError;
use crate::prelude::{Logger, LoggerHandle, SetupLogger};

enum LoggerHandleState {
    Unloaded = 0,
    Locked = 1,
    Ready = 2,
    Poisoned = 3,
}

pub(crate) struct Lazy<H: SetupLogger<T, LoggerError>, T> {
    inner: core::cell::UnsafeCell<Option<T>>,
    state: AtomicU8,
    _handle_phantom: core::marker::PhantomData<H>,
}

impl<H, T> Lazy<H, T>
where
    H: SetupLogger<T, LoggerError>,
{
    /// Get or initialize the value.
    ///
    /// This function returns the inner value if it's already initialized,
    /// or it initializes and returns it if not.
    pub async fn get_or_init(
        &self, log_group_name: &'static str, log_stream_name: &'static str,
        batch_size: usize, interval: Duration
    ) -> Result<&T, LoggerError> {
        if self.state.load(Ordering::Acquire) == LoggerHandleState::Ready as u8 {
            // SAFETY: This is safe because we are the only thread that can mutate the inner
            // pointer, and we are holding a lock.
            unsafe {
                return (*self.inner.get()).as_ref().ok_or(LoggerError::Poisoned);
            }
        }

        match self.state.compare_exchange(
            LoggerHandleState::Unloaded as u8,
            LoggerHandleState::Locked as u8,
            Ordering::Acquire,
            Ordering::Relaxed
        ) {
            Ok(_) => {
                let result = H::setup(
                    log_group_name, log_stream_name, batch_size, interval
                ).await;

                match result {
                    Ok(value) => {
                        // Store the successfully initialized logger.
                        unsafe { *self.inner.get() = Some(value); }
                        self.state.store(LoggerHandleState::Ready as u8, Ordering::Release);
                        Ok(unsafe { (*self.inner.get()).as_ref().unwrap() })
                    },
                    Err(e) => {
                        // Mark the logger as poisoned.
                        self.state.store(LoggerHandleState::Poisoned as u8, Ordering::Release);
                        Err(e)
                    }
                }
            }

            Err(_) => {
                if self.state.load(Ordering::Acquire) == LoggerHandleState::Poisoned as u8 {
                    return Err(LoggerError::Poisoned);
                }
                while self.state.load(Ordering::Acquire) != LoggerHandleState::Ready as u8 {
                    #[cfg(not(all(test, feature = "loom")))]
                    yield_now().await;
                    #[cfg(all(test, feature = "loom"))]
                    yield_now();
                }
                unsafe {
                    Ok((*self.inner.get()).as_ref().ok_or(LoggerError::Poisoned)?)
                }
            }
        }
    }
}

impl Default for Lazy<LoggerHandle, Logger> {
    fn default() -> Self {
        Self {
            inner: core::cell::UnsafeCell::new(None),
            state: AtomicU8::new(LoggerHandleState::Unloaded as u8),
            _handle_phantom: std::marker::PhantomData,
        }
    }
}

#[cfg(not(all(test, feature = "loom")))]
pub(crate) const fn new_lazy<H: SetupLogger<T, LoggerError>, T>() -> Lazy<H, T> {
    Lazy {
        inner: core::cell::UnsafeCell::new(None),
        state: AtomicU8::new(LoggerHandleState::Unloaded as u8),
        _handle_phantom: std::marker::PhantomData,
    }
}

// This is for the LoggerHandle which is a background process, this type is not
// exposed, and should not be used outside of this crate.
unsafe impl Sync for Lazy<LoggerHandle, Logger> {}

#[cfg(all(test, not(feature = "loom")))]
mod tests {
    use super::*;
    use crate::prelude::__tests::{setup, teardown};

    #[tokio::test]
    async fn naive_test() {
        setup().await;

        let handle: Lazy<LoggerHandle, Logger> = new_lazy();

        let _hello = handle.get_or_init(
            "test-group", "test-stream", 2, Duration::from_secs(1)
        ).await.cloned();

        teardown().await;
    }
}

#[cfg(all(test, feature = "loom"))]
mod loom_tests {
    use super::*;
    use crate::prelude::__tests::{setup, teardown};

    use loom::thread;
    use loom::sync::Arc;
    use loom::future::block_on;
    use async_trait::async_trait;


    #[derive(Clone)]
    struct AStructForLoom {
        a_value: u8,
    }

    #[async_trait]
    impl SetupLogger<AStructForLoom, LoggerError> for AStructForLoom {
        async fn setup(
            _log_group_name: &'static str, _log_stream_name: &'static str,
            _batch_size: usize, _interval: Duration
        ) -> Result<AStructForLoom, LoggerError> {
            Ok(AStructForLoom {
                a_value: 1
            })
        }
    }

    impl Default for Lazy<AStructForLoom, AStructForLoom> {
        fn default() -> Self {
            Self {
                inner: core::cell::UnsafeCell::new(None),
                state: AtomicU8::new(LoggerHandleState::Unloaded as u8),
                _handle_phantom: std::marker::PhantomData,
            }
        }
    }

    fn loom_helper(handle_clone: Arc<Lazy<AStructForLoom, AStructForLoom>>) {
        let out = block_on(async {
            handle_clone.get_or_init(
                "test-group", "test-stream", 2, Duration::from_secs(1)
            ).await.cloned().unwrap()
        });
        assert_eq!(out.a_value, 1);
    }

    #[tokio::test]
    async fn test_lazy_logger_handle() {
        setup().await;

        loom::model(|| {
            let handle: Lazy<AStructForLoom, AStructForLoom> = Lazy::default();

            let handle_clone = Arc::new(handle);
            let handle_clone2 = handle_clone.clone();

            let t1 = thread::spawn(move || {
                loom_helper(handle_clone)
            });

            let t2 = thread::spawn(move || {
                loom_helper(handle_clone2)
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });

        teardown().await;
    }
}