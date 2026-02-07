use crate::core::StreamCancelHandle;
use axum::body::Bytes;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A stream wrapper that triggers a cancellation handle when dropped.
/// This allows detecting when the client disconnects (stops consuming the stream).
pub struct DisconnectStream<S> {
    pub stream: S,
    pub cancel_handle: StreamCancelHandle,
}

impl<S, E> Stream for DisconnectStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

impl<S> Drop for DisconnectStream<S> {
    fn drop(&mut self) {
        // Signal cancellation when the stream is dropped
        // This happens when the client disconnects or the stream finishes normally
        // The receiver side should check if the stream finished normally before treating it as a disconnect
        if !self.cancel_handle.is_completed() {
            tracing::debug!("Client disconnect detected - stream cancelled");
        }
        self.cancel_handle.cancel();
    }
}
