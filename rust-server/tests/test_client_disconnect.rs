use futures::StreamExt;
use llm_proxy_rust::api::disconnect::DisconnectStream;
use llm_proxy_rust::core::{build_streaming_request_log_record, cancel::StreamCancelHandle};
use std::time::Duration;

#[tokio::test]
async fn test_stream_cancel_handle() {
    let handle = StreamCancelHandle::new();
    let mut rx = handle.subscribe();

    assert!(!handle.is_cancelled());
    assert!(!*rx.borrow());

    handle.cancel();

    assert!(handle.is_cancelled());
    // Wait for the change to propagate
    let _ = rx.changed().await;
    assert!(*rx.borrow());
}

#[tokio::test]
async fn test_disconnect_stream_triggers_cancel_on_drop() {
    let handle = StreamCancelHandle::new();
    let rx = handle.subscribe();

    // Create a dummy stream
    let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from("test"))]);

    {
        let _disconnect_stream = DisconnectStream {
            stream,
            cancel_handle: handle.clone(),
        };

        // Verify not cancelled yet
        assert!(!handle.is_cancelled());
        assert!(!*rx.borrow());

        // _disconnect_stream goes out of scope here
    }

    // Verify cancelled after drop
    assert!(handle.is_cancelled());
    assert!(*rx.borrow());
}

#[tokio::test]
async fn test_streaming_cancellation_logic() {
    // Simulate the logic in create_sse_stream
    let handle = StreamCancelHandle::new();
    let rx = handle.subscribe();

    // Create a stream that yields items slowly
    let upstream = futures::stream::unfold(0, |state| async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if state < 10 {
            Some((Ok::<_, std::convert::Infallible>(state), state + 1))
        } else {
            None
        }
    });

    // Wrap with cancellation logic (simplified version of create_sse_stream logic)
    let stream_with_cancel = futures::stream::unfold(
        (Box::pin(upstream), rx.clone()),
        |(mut upstream, mut cancel_rx)| async move {
            // We need to use a loop to handle spurious wakeups or non-cancellation changes
            loop {
                // Create cancellation future
                let cancel_future = async {
                    let _ = cancel_rx.changed().await;
                    *cancel_rx.borrow()
                };

                tokio::select! {
                    chunk = upstream.next() => {
                        match chunk {
                            Some(Ok(item)) => return Some((Ok::<_, std::convert::Infallible>(item), (upstream, cancel_rx))),
                            Some(Err(_)) => return None,
                            None => return None,
                        }
                    }
                    cancelled = cancel_future => {
                        if cancelled {
                            // Cancelled
                            return None;
                        }
                        // Not cancelled yet (spurious wakeup), continue loop
                    }
                }
            }
        },
    );

    // Pin the stream so we can call next()
    let mut stream_with_cancel = Box::pin(stream_with_cancel);

    // Consume a few items
    let item1 = stream_with_cancel.next().await;
    assert!(item1.is_some());

    // Trigger cancellation
    handle.cancel();

    // Next item should be None (stream ended due to cancellation)
    // We might need to wait a tiny bit for the select! to pick up the change vs the next stream item
    // but since the stream has a sleep, the cancellation should win.
    let item2 = stream_with_cancel.next().await;
    assert!(item2.is_none());
}

#[test]
fn test_disconnect_and_complete_request_record_consistency() {
    let common = (
        "req_consistency",
        "/v2/chat/completions",
        "test-key",
        "gpt-4",
        "test-gpt-4",
        "MockProvider",
        "openai",
        "openai",
        "anthropic",
        12usize,
        34usize,
        123i32,
        Some(45i32),
        Some("{\"user-agent\":\"test\"}"),
    );

    let complete = build_streaming_request_log_record(
        common.0, common.1, common.2, common.3, common.4, common.5, common.6, common.7, common.8,
        200, common.9, common.10, common.11, common.12, None, common.13,
    );

    let disconnect = build_streaming_request_log_record(
        common.0,
        common.1,
        common.2,
        common.3,
        common.4,
        common.5,
        common.6,
        common.7,
        common.8,
        499,
        common.9,
        common.10,
        common.11,
        common.12,
        Some("client_disconnect"),
        common.13,
    );

    // Shared fields should remain identical between complete/disconnect paths.
    assert_eq!(complete.request_id, disconnect.request_id);
    assert_eq!(complete.endpoint, disconnect.endpoint);
    assert_eq!(complete.credential_name, disconnect.credential_name);
    assert_eq!(complete.model_requested, disconnect.model_requested);
    assert_eq!(complete.model_mapped, disconnect.model_mapped);
    assert_eq!(complete.provider_name, disconnect.provider_name);
    assert_eq!(complete.provider_type, disconnect.provider_type);
    assert_eq!(complete.client_protocol, disconnect.client_protocol);
    assert_eq!(complete.provider_protocol, disconnect.provider_protocol);
    assert_eq!(complete.input_tokens, disconnect.input_tokens);
    assert_eq!(complete.output_tokens, disconnect.output_tokens);
    assert_eq!(complete.total_tokens, disconnect.total_tokens);
    assert_eq!(complete.total_duration_ms, disconnect.total_duration_ms);
    assert_eq!(complete.ttft_ms, disconnect.ttft_ms);
    assert_eq!(complete.request_headers, disconnect.request_headers);
    assert!(complete.is_streaming);
    assert!(disconnect.is_streaming);

    // Status-specific fields should differ as expected.
    assert_eq!(complete.status_code, Some(200));
    assert_eq!(disconnect.status_code, Some(499));
    assert_eq!(complete.error_category, None);
    assert_eq!(
        disconnect.error_category,
        Some("client_disconnect".to_string())
    );
}
