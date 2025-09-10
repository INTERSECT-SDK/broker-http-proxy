use std::sync::Arc;

use tokio::sync::oneshot;

use intersect_ingress_proxy_common::configuration::{get_configuration, Protocol};
use intersect_ingress_proxy_common::protocols::{
    amqp::init::init_amqp_proto_handlers, interfaces::SubscribeProtoHandler,
    mqtt::init::init_mqtt_proto_handlers,
};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

use proxy_http_server::{
    broadcaster::Broadcaster,
    configuration::Settings,
    webapp_server::{build_amqp_webapp, build_mqtt_webapp, WebApplication},
    APPLICATION_NAME,
};

// Muslc has a slow allocator, but we can only use jemalloc on 64-bit systems since jemalloc doesn't support i686.
#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

async fn begin_execution(
    configuration: Settings,
    subscribe_proto_handler: impl SubscribeProtoHandler,
    application: WebApplication,
    broadcaster: Arc<Broadcaster>,
) -> anyhow::Result<()> {
    // How this works:
    // - Pass in the receiver to the broker consumer loop
    // - In the broker consumer loop, use tokio::select! to wait for rx.recv() at key points
    // - After the HTTP server has been shut down, drop the sender from memory, which will trigger an rx.recv() command
    // - This allows us to "finish up" publishing a message to our broker before killing the application.
    let (tx, rx) = oneshot::channel();

    let broker_join_handle = subscribe_proto_handler.begin_subscribe_loop(
        configuration.topic_prefix.clone(),
        broadcaster,
        rx,
    );

    drop(configuration);

    application.run_until_stopped().await?;

    tracing::info!("Application shutting down, please wait for cleanups...");
    drop(tx);
    broker_join_handle.await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration::<Settings>().expect("Failed to read configuration");

    // Start logging
    if configuration.production {
        let subscriber = get_json_subscriber(
            APPLICATION_NAME.into(),
            configuration.log_level.to_string(),
            std::io::stderr,
        );
        init_subscriber(subscriber);
    } else {
        let subscriber = get_pretty_subscriber(configuration.log_level.to_string());
        init_subscriber(subscriber);
    }

    let broadcaster = Broadcaster::new();

    match configuration.broker.protocol {
        Protocol::Amqp => {
            match init_amqp_proto_handlers(&configuration.broker, APPLICATION_NAME).await {
                Ok((publish_proto_handler, subscribe_proto_handler)) => {
                    let web_server = build_amqp_webapp(
                        &configuration,
                        broadcaster.clone(),
                        publish_proto_handler,
                    )
                    .await?;
                    begin_execution(
                        configuration,
                        subscribe_proto_handler,
                        web_server,
                        broadcaster,
                    )
                    .await
                }
                Err(err) => {
                    tracing::error!("AMQP broker: initial verification problem -- {err}");
                    std::process::exit(1);
                }
            }
        }
        Protocol::Mqtt => {
            match init_mqtt_proto_handlers(&configuration.broker, APPLICATION_NAME).await {
                Ok((publish_proto_handler, subscribe_proto_handler)) => {
                    let web_server = build_mqtt_webapp(
                        &configuration,
                        broadcaster.clone(),
                        publish_proto_handler,
                    )
                    .await?;
                    begin_execution(
                        configuration,
                        subscribe_proto_handler,
                        web_server,
                        broadcaster,
                    )
                    .await
                }
                Err(err) => {
                    tracing::error!("MQTT broker: initial verification problem -- {err}");
                    std::process::exit(1);
                }
            }
        }
    }
}
