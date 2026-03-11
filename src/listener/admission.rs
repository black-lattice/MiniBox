use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone)]
pub struct AdmissionControl {
    inner: Arc<AdmissionInner>,
}

#[derive(Debug)]
struct AdmissionInner {
    max_connections: usize,
    active_connections: AtomicUsize,
}

impl AdmissionControl {
    pub fn new(max_connections: usize) -> Self {
        Self {
            inner: Arc::new(AdmissionInner {
                max_connections,
                active_connections: AtomicUsize::new(0),
            }),
        }
    }

    pub fn snapshot(&self) -> AdmissionSnapshot {
        AdmissionSnapshot {
            max_connections: self.inner.max_connections,
            active_connections: self.active_connections(),
        }
    }

    pub fn max_connections(&self) -> usize {
        self.inner.max_connections
    }

    pub fn active_connections(&self) -> usize {
        self.inner.active_connections.load(Ordering::Relaxed)
    }

    pub fn remaining_capacity(&self) -> usize {
        self.max_connections()
            .saturating_sub(self.active_connections())
    }

    pub fn can_accept(&self) -> bool {
        self.active_connections() < self.max_connections()
    }

    pub fn try_acquire(&self) -> Result<AdmissionGuard, AdmissionError> {
        let mut observed = self.inner.active_connections.load(Ordering::Relaxed);

        loop {
            if observed >= self.inner.max_connections {
                return Err(AdmissionError::LimitReached {
                    max_connections: self.inner.max_connections,
                });
            }

            match self.inner.active_connections.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(AdmissionGuard {
                        inner: Arc::clone(&self.inner),
                    });
                }
                Err(current) => observed = current,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionSnapshot {
    pub max_connections: usize,
    pub active_connections: usize,
}

impl AdmissionSnapshot {
    pub fn remaining_capacity(&self) -> usize {
        self.max_connections.saturating_sub(self.active_connections)
    }
}

#[derive(Debug)]
pub struct AdmissionGuard {
    inner: Arc<AdmissionInner>,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        self.inner.active_connections.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    LimitReached { max_connections: usize },
}

impl Display for AdmissionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitReached { max_connections } => {
                write!(f, "connection limit reached at {max_connections}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AdmissionControl, AdmissionError};

    #[test]
    fn guard_updates_active_connection_count() {
        let control = AdmissionControl::new(2);
        assert_eq!(control.snapshot().remaining_capacity(), 2);

        let first = control.try_acquire().expect("first guard should acquire");
        let second = control.try_acquire().expect("second guard should acquire");

        assert_eq!(control.active_connections(), 2);
        assert!(matches!(
            control.try_acquire(),
            Err(AdmissionError::LimitReached { max_connections: 2 })
        ));

        drop(first);
        assert_eq!(control.active_connections(), 1);

        drop(second);
        assert_eq!(control.active_connections(), 0);
    }
}
