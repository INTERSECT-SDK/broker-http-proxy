//! src/startup.rs
// see: https://github.com/tokio-rs/axum/blob/main/examples/sqlx-postgres/src/main.rs

use axum::{routing::get, serve::Serve, Router};
use secrecy::Secret;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    request_id::MakeRequestUuid,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    ServiceBuilderExt,
};
use tracing::Level;

use crate::{
    amqp_manager::AmqpManager,
    broadcaster::Broadcaster,
    configuration::Settings,
    routes::{health_check::health_check, not_found::handler_404, subscribe::sse_handler},
};

use intersect_ingress_proxy_common::signals::wait_for_os_signal;

/// This is state that can be accessed by any endpoint on the server.
pub struct ApplicationState {
    /// this broadcaster gets messages published to it from one source and can publish many messages from it
    pub broadcaster: Arc<Broadcaster>,
    /// basic auth username
    pub username: String,
    /// basic auth password
    pub password: Secret<String>,
}

type AppServer = Serve<Router, Router>;
pub struct Application {
    pub port: u16,
    pub server: AppServer,
}

impl Application {
    pub async fn build(configuration: Settings) -> Result<(Self, AmqpManager), anyhow::Error> {
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
        let (server, amqp_manager) = run(listener, &configuration).await?;

        tracing::info!("Web server is running on port {}", port);

        Ok((Self { port, server }, amqp_manager))
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
) -> Result<(AppServer, AmqpManager), anyhow::Error> {
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

    let broadcaster = Broadcaster::new();
    // TODO this is a slightly awkward way to persist the AMQPManager
    let amqp_manager = AmqpManager::new(configuration, broadcaster.clone()).await;
    let app_state = Arc::new(ApplicationState {
        broadcaster,
        username: configuration.username.clone(),
        password: configuration.password.clone(),
    });

    let app = Router::new()
        .route("/healthcheck", get(health_check))
        .route("/", get(sse_handler))
        .layer(middleware)
        .with_state(app_state)
        .fallback(handler_404);

    let server = axum::serve(listener, app);
    Ok((server, amqp_manager))
}
