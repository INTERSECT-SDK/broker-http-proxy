use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    TypedHeader,
};
use futures::stream::Stream;
use secrecy::ExposeSecret;
use std::convert::Infallible;
use std::sync::Arc;

use crate::webapp::WebApplicationState;
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

fn sse_response(
    app_state: Arc<WebApplicationState>,
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
                // TODO figure out more robust mechanism to handle "lagged" errors from the receiver.
                resp = rx.recv() => {
                    match resp {
                        Ok(event) => {
                            yield Ok(event);
                        },
                        Err(e) => {
                            match e {
                                tokio::sync::broadcast::error::RecvError::Closed => {
                                    tracing::error!(error = ?e, "Broadcasting pipeline to SSE somehow closed, should not see this message!")
                                },
                                tokio::sync::broadcast::error::RecvError::Lagged(lag_count) => {
                                    tracing::error!(error = ?e, "SSE has missed {} messages from broadcaster", lag_count)
                                },
                            };
                        },
                    }
                },
            };
        };
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Resources:
/// https://github.com/tokio-rs/axum/discussions/1670
/// https://github.com/tokio-rs/axum/discussions/2264
pub async fn sse_handler(
    State(app_state): State<Arc<WebApplicationState>>,
    TypedHeader(authorization): TypedHeader<Authorization<Basic>>,
) -> impl IntoResponse {
    if authorization.username() != app_state.username
        || authorization.password() != app_state.password.expose_secret()
    {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    sse_response(app_state).into_response()
}
