use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    TypedHeader,
};
use secrecy::ExposeSecret;
use std::sync::Arc;

use intersect_ingress_proxy_common::intersect_messaging::extract_eventsource_data;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_channel, is_routing_key_compliant, publish::amqp_publish_message,
};

use crate::webapp::WebApplicationState;

/// HTTP POST endpoint which will publish a message meeting the INTERSECT specification
pub async fn publish_message(
    State(app_state): State<Arc<WebApplicationState>>,
    TypedHeader(authorization): TypedHeader<Authorization<Basic>>,
    request: Request,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if authorization.username() != app_state.username
        || authorization.password() != app_state.password.expose_secret()
    {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }

    let bytes = to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| (StatusCode::BAD_REQUEST, "garbled body".to_string()))?;

    // TODO - this is meant for backwards compatibility with the existing logic tailor made for SSEs
    // We should eventually switch to a format (Websockets) which allows for non-UTF8 data to be sent in the request (we only care about the raw bytes anyways)
    let strng = String::from_utf8(bytes.to_vec())
        .map_err(|_| (StatusCode::BAD_REQUEST, "body is not utf-8".to_string()))?;

    let (topic, data) = extract_eventsource_data(&strng).map_err(|e| {
        tracing::warn!("body is not valid INTERSECT format: {}", e);
        (
            StatusCode::BAD_REQUEST,
            "body is not valid INTERSECT format".to_string(),
        )
    })?;
    if !is_routing_key_compliant(&topic) {
        tracing::warn!(
            "{} is not a valid AMQP topic name, will not attempt publish",
            topic
        );
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{} is not a valid AMQP topic name", topic),
        ));
    }
    tracing::debug!("Publishing message with topic: {}", &topic);

    let connection = app_state.amqp_connection_pool.get().await.map_err(|e| {
        tracing::error!(error = ?e, "cannot connect to broker");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server fault, message not published".to_string(),
        )
    })?;

    let channel = get_channel(&connection).await.map_err(|e| {
        tracing::error!(error = ?e, "cannot create channel on broker");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server fault, message not published".to_string(),
        )
    })?;
    amqp_publish_message(channel, &topic, data)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server fault, message not published".to_string(),
            )
        })?;

    Ok((StatusCode::CREATED, "Success".to_string()))
}
