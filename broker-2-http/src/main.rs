use std::sync::Arc;

use broker_2_http::{
    amqp_consumer::broker_consumer_loop, broadcaster::Broadcaster, configuration::Settings,
    webapp::WebApplication,
};

use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_connection_pool, verify_connection_pool,
};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};
use tokio::sync::Barrier;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration::<Settings>().expect("Failed to read configuration");

    // Start logging
    if configuration.production {
        let subscriber = get_json_subscriber(
            "broker-2-http".into(),
            configuration.log_level.to_string(),
            std::io::stderr,
        );
        init_subscriber(subscriber);
    } else {
        let subscriber = get_pretty_subscriber(configuration.log_level.to_string());
        init_subscriber(subscriber);
    }

    // set up broker connection pool
    let pool = get_connection_pool(&configuration.broker).await;
    if let Err(msg) = verify_connection_pool(&pool).await {
        tracing::error!(msg);
        std::process::exit(1);
    }

    let broadcaster = Broadcaster::new();
    let application =
        WebApplication::build(&configuration, broadcaster.clone(), pool.clone()).await?;

    let barrier = Arc::new(Barrier::new(2));

    let _broker_join_handle = broker_consumer_loop(
        pool,
        configuration.topic_prefix.clone(),
        broadcaster.clone(),
        Arc::clone(&barrier),
    )
    .await;

    application.run_until_stopped().await?;
    tracing::warn!("Application shutting down, please wait for cleanups...");
    barrier.wait().await;
    // tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    // broker_join_handle.abort();

    Ok(())
}
