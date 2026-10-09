//! Optional counters for the original draw benchmark. Provenance: 0036.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static NANOS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
static CALLS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
/// Pipeline lookup, descriptor preparation, and uniform resolution totals.
pub fn take() -> [(u64, u64); 3] {
    std::array::from_fn(|i| {
        (
            NANOS[i].swap(0, Ordering::Relaxed),
            CALLS[i].swap(0, Ordering::Relaxed),
        )
    })
}
pub(crate) struct Span(usize, Instant);
impl Span {
    pub(crate) fn new(index: usize) -> Self {
        Self(index, Instant::now())
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        NANOS[self.0].fetch_add(self.1.elapsed().as_nanos() as u64, Ordering::Relaxed);
        CALLS[self.0].fetch_add(1, Ordering::Relaxed);
    }
}
