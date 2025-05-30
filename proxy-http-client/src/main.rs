use std::sync::Arc;

use tokio::sync::oneshot;

use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_connection_pool, subscribe::broker_consumer_loop, verify_connection_pool,
};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

use proxy_http_client::{configuration::Settings, event_source::event_source_loop, poster::Poster};

const APPLICATION_NAME: &str = "proxy-http-client";

// Muslc has a slow allocator, but we can only use jemalloc on 64-bit systems since jemalloc doesn't support i686.
#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
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

    // set up broker connection pool
    let pool = get_connection_pool(&configuration.broker);
    if let Err(msg) = verify_connection_pool(&pool, APPLICATION_NAME).await {
        tracing::error!(msg);
        std::process::exit(1);
    }

    // How this works:
    // - Pass in the receiver to the broker consumer loop
    // - In the broker consumer loop, use tokio::select! to wait for rx.recv() at key points
    // - After the Event Source loop has been shut down (either because we're killing the app or because we got a server error),
    //     drop the sender from memory, which will trigger an rx.recv() command
    // - This allows us to "finish up" publishing a message to our broker before killing the application.
    let (tx, rx) = oneshot::channel();

    let broker_join_handle = broker_consumer_loop(
        pool.clone(),
        configuration.topic_prefix.clone(),
        APPLICATION_NAME.into(),
        Arc::new(Poster::new(&configuration.other_proxy)),
        rx,
    );

    let other_proxy = configuration.other_proxy.clone();
    drop(configuration);

    // this will run until we get an event source error or we catch an OS signal
    let rc = event_source_loop(other_proxy, pool).await;

    tracing::info!("Attempting graceful shutdown: No longer listening for events over HTTP");
    drop(tx);
    broker_join_handle.await?;

    std::process::exit(rc);
}
