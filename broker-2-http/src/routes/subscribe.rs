use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;

use crate::startup::ApplicationState;
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

/// Resources:
/// https://github.com/tokio-rs/axum/discussions/1670
/// https://github.com/tokio-rs/axum/discussions/2264
pub async fn sse_handler(
    State(app_state): State<Arc<ApplicationState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = app_state.broadcaster.add_client();

    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                // if we catch an OS signal, disconnect the client
                _ = wait_for_os_signal() => {
                    break;
                },
                // send the broadcast message to the client, and continue listening for more messages
                resp = rx.recv() => {
                    match resp {
                        Ok(event) => {
                            yield Ok(event);
                        },
                        Err(e) => {
                            tracing::error!(error = ?e, "SSE somehow didn't get message from broadcaster");
                        },
                    }
                },
            };
        };
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
