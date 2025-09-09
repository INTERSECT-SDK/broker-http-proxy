use axum::{
    routing::{get, post},
    serve::Serve,
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    request_id::MakeRequestUuid,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    ServiceBuilderExt,
};
use tracing::Level;

use intersect_ingress_proxy_common::protocols::{
    amqp::publish::AmqpPublishProtoHandler, mqtt::publish::MqttPublishProtoHandler,
};
use intersect_ingress_proxy_common::server_paths::{PUBLISH_URL, SUBSCRIBE_URL};
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

use crate::{
    broadcaster::Broadcaster,
    configuration::Settings,
    routes::{
        health_check::health_check, not_found::handler_404, publish::publish_message,
        subscribe::sse_handler,
    },
    webapp_state::{AmqpWebApplicationState, MqttWebApplicationState, WebApplicationState},
};

type WebAppServer = Serve<TcpListener, Router, Router>;

fn get_router<S: WebApplicationState + Send + Sync + 'static>(initial_state: S) -> Router {
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
    Router::new()
        .route(SUBSCRIBE_URL, get(sse_handler))
        .route(PUBLISH_URL, post(publish_message))
        .layer(middleware)
        .with_state(Arc::new(initial_state))
        .route("/healthcheck", get(health_check))
        .fallback(handler_404)
}

pub trait WebApplication {
    fn port(&self) -> u16;
    fn run_until_stopped(
        self,
    ) -> impl std::future::Future<Output = Result<(), std::io::Error>> + Send;
}

pub struct AmqpWebApplication {
    port: u16,
    server: WebAppServer,
}

impl AmqpWebApplication {
    ///
    /// # Errors
    ///   - errors if unable to bind to provided TCP port
    pub async fn build(
        configuration: &Settings,
        broadcaster: Arc<Broadcaster>,
        proto_handler: AmqpPublishProtoHandler,
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
        let port = listener.local_addr()?.port();
        let router = get_router(AmqpWebApplicationState {
            proto_handler: proto_handler.clone(),
            broadcaster,
            username: configuration.username.clone(),
            password: configuration.password.clone(),
        });
        let server = axum::serve(listener, router);

        tracing::info!("Web server is running on port {}", port);

        Ok(Self { port, server })
    }
}

impl WebApplication for AmqpWebApplication {
    fn port(&self) -> u16 {
        self.port
    }

    ///
    /// # Errors
    ///   - Errors if unable to initialize web server
    async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        // the return type of "with_graceful_shutdown" is unstable, so set it up here
        self.server
            .with_graceful_shutdown(wait_for_os_signal())
            .await
    }
}

pub struct MqttWebApplication {
    port: u16,
    server: WebAppServer,
}

impl MqttWebApplication {
    ///
    /// # Errors
    ///   - errors if unable to bind to provided TCP port
    pub async fn build(
        configuration: &Settings,
        broadcaster: Arc<Broadcaster>,
        proto_handler: MqttPublishProtoHandler,
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
        let port = listener.local_addr()?.port();
        let router = get_router(MqttWebApplicationState {
            proto_handler: proto_handler.clone(),
            broadcaster,
            username: configuration.username.clone(),
            password: configuration.password.clone(),
        });
        let server = axum::serve(listener, router);

        tracing::info!("Web server is running on port {}", port);

        Ok(Self { port, server })
    }
}

impl WebApplication for MqttWebApplication {
    fn port(&self) -> u16 {
        self.port
    }

    ///
    /// # Errors
    ///   - Errors if unable to initialize web server
    async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        // the return type of "with_graceful_shutdown" is unstable, so set it up here
        self.server
            .with_graceful_shutdown(wait_for_os_signal())
            .await
    }
}
