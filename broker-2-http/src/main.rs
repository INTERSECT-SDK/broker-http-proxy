use broker_2_http::{
    amqp_consumer::broker_consumer_loop, broadcaster::Broadcaster, configuration::Settings,
    webapp::WebApplication,
};

use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

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

    let broadcaster = Broadcaster::new();

    let application = WebApplication::build(&configuration, broadcaster.clone()).await?;

    let broker_join_handle = broker_consumer_loop(
        configuration.broker.clone(),
        configuration.topic_prefix.clone(),
        broadcaster.clone(),
    )
    .await;
    application.run_until_stopped().await?;
    tracing::warn!("Application shutting down, please wait for cleanups...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    broker_join_handle.abort();
    Ok(())
}
