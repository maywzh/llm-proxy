use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

/// Handle for cancelling streaming operations when client disconnects
#[derive(Clone)]
pub struct StreamCancelHandle {
    sender: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
    /// Flag to track if stream completed normally (not a disconnect)
    completed: Arc<AtomicBool>,
}

impl StreamCancelHandle {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender,
            receiver,
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark the stream as completed normally.
    /// This should be called when the stream ends successfully.
    pub fn mark_completed(&self) {
        self.completed.store(true, Ordering::SeqCst);
    }

    /// Check if the stream completed normally
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }

    /// Signal cancellation (only if not already completed)
    pub fn cancel(&self) {
        // Only signal cancellation if the stream hasn't completed normally
        if !self.is_completed() {
            let _ = self.sender.send(true);
        }
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Get a receiver for use in select!
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.receiver.clone()
    }
}

impl Default for StreamCancelHandle {
    fn default() -> Self {
        Self::new()
    }
}
