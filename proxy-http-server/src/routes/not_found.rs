use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn handler_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not an endpoint")
}
