use tokio::sync::oneshot;

use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::protocols::amqp::subscribe::broker_consumer_loop;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_connection_pool, verify_connection_pool,
};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

use proxy_http_server::{
    broadcaster::Broadcaster, configuration::Settings, webapp::WebApplication,
};

const APPLICATION_NAME: &str = "proxy-http-server";

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

    // set up broker connection pool
    let pool = get_connection_pool(&configuration.broker);
    if let Err(msg) = verify_connection_pool(&pool, APPLICATION_NAME).await {
        tracing::error!(msg);
        std::process::exit(1);
    }

    let broadcaster = Broadcaster::new();
    let application =
        WebApplication::build(&configuration, broadcaster.clone(), pool.clone()).await?;

    // How this works:
    // - Pass in the receiver to the broker consumer loop
    // - In the broker consumer loop, use tokio::select! to wait for rx.recv() at key points
    // - After the HTTP server has been shut down, drop the sender from memory, which will trigger an rx.recv() command
    // - This allows us to "finish up" publishing a message to our broker before killing the application.
    let (tx, rx) = oneshot::channel();

    let broker_join_handle = broker_consumer_loop(
        pool,
        configuration.topic_prefix.clone(),
        APPLICATION_NAME.into(),
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
