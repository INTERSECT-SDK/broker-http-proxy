use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    TypedHeader,
};
use intersect_ingress_proxy_common::protocols::ProtoHandler;
use secrecy::ExposeSecret;
use std::sync::Arc;

use intersect_ingress_proxy_common::intersect_messaging::extract_eventsource_data;

use crate::webapp_state::WebApplicationState;

/// HTTP POST endpoint which will publish a message meeting the INTERSECT specification
///
/// # Errors
///   - Sends back a 401 if authentication is incorrect
///   - Sends back a 400 if the message body is improperly formatted
///   - Sends back a 500 if the server was unable to send the message
pub async fn publish_message(
    State(app_state): State<Arc<impl WebApplicationState>>,
    TypedHeader(authorization): TypedHeader<Authorization<Basic>>,
    request: Request,
) -> Result<(StatusCode, String), (StatusCode, String)> {
    if authorization.username() != app_state.get_username()
        || authorization.password() != app_state.get_password().expose_secret()
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

    if let Err(e) = app_state.get_proto_handler().preverify_publish(&topic) {
        return Err((StatusCode::BAD_REQUEST, e));
    }

    tracing::debug!("Publishing message with topic: {}", &topic);

    match app_state
        .get_proto_handler()
        .publish_message(&topic, data)
        .await
    {
        Ok(_) => Ok((StatusCode::CREATED, "Success".to_string())),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "server fault, message not published".to_string(),
        )),
    }
}
