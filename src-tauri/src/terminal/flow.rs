use std::sync::{Condvar, Mutex, MutexGuard};

use thiserror::Error;

pub const OUTPUT_BATCH_BYTES: usize = 32 * 1024;
pub const OUTPUT_HIGH_WATER_BYTES: usize = 256 * 1024;
pub const OUTPUT_RESUME_BYTES: usize = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlowError {
    #[error("the output stream is closed")]
    Closed,
    #[error("the output acknowledgement is invalid")]
    InvalidAcknowledgement,
    #[error("the output reservation is invalid")]
    InvalidReservation,
}

#[derive(Debug, Default)]
struct FlowState {
    unacknowledged: usize,
    paused: bool,
    closed: bool,
}

#[derive(Debug, Default)]
pub struct OutputFlow {
    state: Mutex<FlowState>,
    changed: Condvar,
}

impl OutputFlow {
    /// Blocks a PTY reader until it may read another bounded batch.
    pub fn reserve(&self) -> Result<usize, FlowError> {
        let mut state = self.lock_state();

        loop {
            if state.closed {
                return Err(FlowError::Closed);
            }

            if state.paused {
                if state.unacknowledged <= OUTPUT_RESUME_BYTES {
                    state.paused = false;
                } else {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    continue;
                }
            }

            let available = OUTPUT_HIGH_WATER_BYTES.saturating_sub(state.unacknowledged);
            if available == 0 {
                state.paused = true;
                continue;
            }

            return Ok(available.min(OUTPUT_BATCH_BYTES));
        }
    }

    pub fn commit(&self, bytes: usize) -> Result<(), FlowError> {
        let mut state = self.lock_state();
        if state.closed {
            return Err(FlowError::Closed);
        }
        if bytes == 0
            || bytes > OUTPUT_BATCH_BYTES
            || bytes > OUTPUT_HIGH_WATER_BYTES.saturating_sub(state.unacknowledged)
        {
            return Err(FlowError::InvalidReservation);
        }

        state.unacknowledged += bytes;
        if state.unacknowledged >= OUTPUT_HIGH_WATER_BYTES {
            state.paused = true;
        }
        Ok(())
    }

    pub fn acknowledge(&self, bytes: usize) -> Result<(), FlowError> {
        let mut state = self.lock_state();
        if bytes == 0 || bytes > state.unacknowledged {
            return Err(FlowError::InvalidAcknowledgement);
        }

        state.unacknowledged -= bytes;
        self.changed.notify_all();
        Ok(())
    }

    pub fn close(&self) {
        let mut state = self.lock_state();
        state.closed = true;
        self.changed.notify_all();
    }

    /// Releases credit owned by a webview that handed the session to another
    /// webview. The replacement receives a replay snapshot outside this flow.
    pub fn reset(&self) {
        let mut state = self.lock_state();
        if state.closed {
            return;
        }
        state.unacknowledged = 0;
        state.paused = false;
        self.changed.notify_all();
    }

    fn lock_state(&self) -> MutexGuard<'_, FlowState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn outstanding(&self) -> usize {
        self.lock_state().unacknowledged
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crossbeam_channel::bounded;

    use super::*;

    #[test]
    fn caps_each_reservation_and_total_outstanding_bytes() {
        let flow = OutputFlow::default();
        assert_eq!(flow.reserve().unwrap(), OUTPUT_BATCH_BYTES);

        for _ in 0..(OUTPUT_HIGH_WATER_BYTES / OUTPUT_BATCH_BYTES) {
            let reserved = flow.reserve().unwrap();
            flow.commit(reserved).unwrap();
        }

        assert_eq!(flow.outstanding(), OUTPUT_HIGH_WATER_BYTES);
    }

    #[test]
    fn resumes_only_after_crossing_the_low_water_mark() {
        let flow = Arc::new(OutputFlow::default());
        for _ in 0..(OUTPUT_HIGH_WATER_BYTES / OUTPUT_BATCH_BYTES) {
            flow.commit(OUTPUT_BATCH_BYTES).unwrap();
        }

        let (ready_tx, ready_rx) = bounded(1);
        let reader_flow = Arc::clone(&flow);
        let reader = std::thread::spawn(move || {
            ready_tx.send(reader_flow.reserve()).unwrap();
        });

        flow.acknowledge(OUTPUT_RESUME_BYTES).unwrap();
        assert!(ready_rx.recv_timeout(Duration::from_millis(40)).is_err());

        flow.acknowledge(OUTPUT_HIGH_WATER_BYTES - (2 * OUTPUT_RESUME_BYTES))
            .unwrap();
        assert_eq!(
            ready_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(OUTPUT_BATCH_BYTES)
        );
        reader.join().unwrap();
    }

    #[test]
    fn rejects_zero_and_excessive_acknowledgements() {
        let flow = OutputFlow::default();
        flow.commit(10).unwrap();

        assert_eq!(flow.acknowledge(0), Err(FlowError::InvalidAcknowledgement));
        assert_eq!(flow.acknowledge(11), Err(FlowError::InvalidAcknowledgement));
        flow.acknowledge(10).unwrap();
        assert_eq!(flow.outstanding(), 0);
    }

    #[test]
    fn closing_releases_a_paused_reader() {
        let flow = Arc::new(OutputFlow::default());
        for _ in 0..(OUTPUT_HIGH_WATER_BYTES / OUTPUT_BATCH_BYTES) {
            flow.commit(OUTPUT_BATCH_BYTES).unwrap();
        }

        let (ready_tx, ready_rx) = bounded(1);
        let reader_flow = Arc::clone(&flow);
        let reader = std::thread::spawn(move || {
            ready_tx.send(reader_flow.reserve()).unwrap();
        });
        flow.close();

        assert_eq!(
            ready_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err(FlowError::Closed)
        );
        reader.join().unwrap();
    }
}
