use axum::{routing::get, routing::post, serve::Serve, Router};
use deadpool_amqprs::Pool;
use secrecy::SecretString;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    request_id::MakeRequestUuid,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    ServiceBuilderExt,
};
use tracing::Level;

use intersect_ingress_proxy_common::server_paths::{PUBLISH_URL, SUBSCRIBE_URL};
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

use crate::{
    broadcaster::Broadcaster,
    configuration::Settings,
    routes::{
        health_check::health_check, not_found::handler_404, publish::publish_message,
        subscribe::sse_handler,
    },
};

/// This is state that can be accessed by any endpoint on the server.
pub struct WebApplicationState {
    /// AMQP connection pool for publishing to the broker.
    pub amqp_connection_pool: Pool,
    /// this broadcaster gets messages published to it from one source and can publish many messages from it. Use this if an HTTP endpoint needs to react to a subscription.
    pub broadcaster: Arc<Broadcaster>,
    /// basic auth username
    pub username: String,
    /// basic auth password
    pub password: SecretString,
}

type WebAppServer = Serve<TcpListener, Router, Router>;
pub struct WebApplication {
    port: u16,
    server: WebAppServer,
}

impl WebApplication {
    pub async fn build(
        configuration: &Settings,
        broadcaster: Arc<Broadcaster>,
        amqp_pool: Pool,
    ) -> Result<Self, anyhow::Error> {
        let address = format!(
            "{}:{}",
            if configuration.production {
                "0.0.0.0"
            } else {
                "127.0.0.1"
            },
            configuration.app_port
        );
        let listener = TcpListener::bind(address).await?;
        let port = listener.local_addr().unwrap().port();
        let server = run(listener, configuration, broadcaster, amqp_pool).await?;

        tracing::info!("Web server is running on port {}", port);

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        // the return type of "with_graceful_shutdown" is unstable, so set it up here
        self.server
            .with_graceful_shutdown(wait_for_os_signal())
            .await
    }
}

async fn run(
    listener: TcpListener,
    configuration: &Settings,
    broadcaster: Arc<Broadcaster>,
    amqp_pool: Pool,
) -> Result<WebAppServer, anyhow::Error> {
    let middleware = ServiceBuilder::new()
        .set_x_request_id(MakeRequestUuid)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(
                    DefaultMakeSpan::new()
                        .include_headers(true)
                        .level(Level::INFO),
                )
                .on_response(DefaultOnResponse::new().include_headers(true)),
        )
        .propagate_x_request_id();

    let app_state = Arc::new(WebApplicationState {
        amqp_connection_pool: amqp_pool,
        broadcaster,
        username: configuration.username.clone(),
        password: configuration.password.clone(),
    });

    let app = Router::new()
        .route(SUBSCRIBE_URL, get(sse_handler))
        .route(PUBLISH_URL, post(publish_message))
        .layer(middleware) // routes added before this layer will be logged, after this layer will not be logged
        .with_state(app_state)
        .route("/healthcheck", get(health_check))
        .fallback(handler_404);

    let server = axum::serve(listener, app);
    Ok(server)
}
