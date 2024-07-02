use broker_2_http::{configuration::get_configuration, startup::Application};
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration().expect("Failed to read configuration");

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

    let (application, amqp_manager) = Application::build(configuration).await?;
    application.run_until_stopped().await?;
    tracing::warn!("Application shutting down, please wait for cleanups...");
    amqp_manager.destruct().await;
    Ok(())
}
