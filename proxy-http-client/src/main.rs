use std::sync::Arc;

use tokio::sync::oneshot;

use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::protocols::{
    amqp::init::init_amqp_proto_handlers,
    interfaces::{PublishProtoHandler, SubscribeProtoHandler},
    mqtt::init::init_mqtt_proto_handlers,
};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

use proxy_http_client::{
    configuration::Settings, event_source::event_source_loop, poster::Poster, APPLICATION_NAME,
};

// Muslc has a slow allocator, but we can only use jemalloc on 64-bit systems since jemalloc doesn't support i686.
#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

async fn begin_execution(
    configuration: Settings,
    publish_proto_handler: impl PublishProtoHandler,
    subscribe_proto_handler: impl SubscribeProtoHandler,
) -> anyhow::Result<()> {
    // How this works:
    // - Pass in the receiver to the broker consumer loop
    // - In the broker consumer loop, use tokio::select! to wait for rx.recv() at key points
    // - After the Event Source loop has been shut down (either because we're killing the app or because we got a server error),
    //     drop the sender from memory, which will trigger an rx.recv() command
    // - This allows us to "finish up" publishing a message to our broker before killing the application.
    let (tx, rx) = oneshot::channel();

    let broker_join_handle = subscribe_proto_handler.begin_subscribe_loop(
        configuration.topic_prefix.clone(),
        Arc::new(Poster::new(&configuration.other_proxy)),
        rx,
    );

    let other_proxy = configuration.other_proxy.clone();
    drop(configuration);

    // this will run until we get an event source error or we catch an OS signal
    let rc = event_source_loop(other_proxy, publish_proto_handler).await;

    tracing::info!("Attempting graceful shutdown: No longer listening for events over HTTP");
    drop(tx);
    broker_join_handle.await?;

    std::process::exit(rc);
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

    match configuration.broker.protocol {
        intersect_ingress_proxy_common::configuration::Protocol::Amqp => {
            match init_amqp_proto_handlers(&configuration.broker, APPLICATION_NAME).await {
                Ok((publish_proto_handler, subscribe_proto_handler)) => {
                    begin_execution(
                        configuration,
                        publish_proto_handler,
                        subscribe_proto_handler,
                    )
                    .await
                }
                Err(err) => {
                    tracing::error!("AMQP broker: initial verification problem -- {err}");
                    std::process::exit(1);
                }
            }
        }
        intersect_ingress_proxy_common::configuration::Protocol::Mqtt => {
            match init_mqtt_proto_handlers(&configuration.broker, APPLICATION_NAME).await {
                Ok((publish_proto_handler, subscribe_proto_handler)) => {
                    begin_execution(
                        configuration,
                        publish_proto_handler,
                        subscribe_proto_handler,
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
