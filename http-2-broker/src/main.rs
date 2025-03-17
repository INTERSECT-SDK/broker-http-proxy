use std::sync::Arc;

use deadpool_amqprs::Pool;
use futures::StreamExt;
use http_2_broker::poster::Poster;
use reqwest_eventsource::{Event, EventSource};
use secrecy::ExposeSecret;
use tokio::sync::oneshot;

use http_2_broker::configuration::Settings;
use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::intersect_messaging::extract_eventsource_data;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_channel, get_connection_pool, is_routing_key_compliant, publish::amqp_publish_message,
    subscribe::broker_consumer_loop, verify_connection_pool,
};
use intersect_ingress_proxy_common::server_paths::SUBSCRIBE_URL;
use intersect_ingress_proxy_common::signals::wait_for_os_signal;
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

/// Return Err only if we weren't able to publish a correct message to the broker, invalid messages are ignored
async fn send_message(message: String, connection_pool: Pool) -> Result<(), String> {
    let es_data_result = extract_eventsource_data(&message);
    if es_data_result.is_err() {
        return Ok(());
    }
    let (topic, data) = es_data_result.unwrap();
    if !is_routing_key_compliant(&topic) {
        tracing::warn!(
            "{} is not a valid AMQP topic name, will not attempt publish",
            topic
        );
        return Ok(());
    }
    tracing::debug!("Publishing message with topic: {}", &topic);

    let connection = connection_pool.get().await.map_err(|_| {
        "WARNING: Couldn't get connection, message received from other proxy was NOT published on our own broker."
            .to_string()
    })?;

    let channel = get_channel(&connection).await.map_err(|_| {
        "WARNING: Couldn't get channel, message received from other proxy was NOT published on our own broker."
            .to_string()
    })?;

    match amqp_publish_message(channel, &topic, data).await {
        Ok(_) => Ok(()),
        Err(_) => Err(
            "WARNING: message received from other proxy was NOT published on our own broker."
                .into(),
        ),
    }
}

/// Return value - exit code to use
async fn event_source_loop(configuration: &Settings, connection_pool: Pool) -> i32 {
    let mut es = EventSource::new(
        reqwest::Client::new()
            .get(format!(
                "{}{}",
                &configuration.other_proxy.url, SUBSCRIBE_URL
            ))
            .basic_auth(
                &configuration.other_proxy.username,
                Some(configuration.other_proxy.password.expose_secret()),
            ),
    )
    .unwrap();
    let mut rc = 0;
    loop {
        tokio::select! {
            // got data back from web server
            evt = es.next() => {
                match evt {
                    None => {
                        // probably isn't reachable
                        tracing::error!("couldn't get next event");
                        rc = 1;
                        break;
                    },
                    Some(event) => {
                        match event {
                            Ok(Event::Open) => {
                                tracing::info!("connected to {}", &configuration.other_proxy.url);
                            },
                            Ok(Event::Message(message)) => {
                                if let Err(e) = send_message(message.data, connection_pool.clone()).await {
                                    tracing::error!(e);
                                };
                            },
                            Err(err) => {
                                // will happen if we can't connect to the endpoint OR if the endpoint drops us
                                tracing::error!(error = ?err, "Event source error --- {}", err);
                                rc = 1;
                                break;
                            },
                        }
                    },
                }
            },
            // OS kill signal
            _ = wait_for_os_signal() => {
                break;
            },
        };
    }
    es.close();

    rc
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration::<Settings>().expect("Failed to read configuration");

    // Start logging
    if configuration.production {
        let subscriber = get_json_subscriber(
            "http-2-broker".into(),
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

    // How this works:
    // - Pass in the receiver to the broker consumer loop
    // - In the broker consumer loop, use tokio::select! to wait for rx.recv() at key points
    // - After the Event Source loop has been shut down (either because we're killing the app or because we got a server error),
    //     drop the sender from memory, which will trigger an rx.recv() command
    // - This allows us to "finish up" publishing a message to our broker before killing the application.
    let (tx, rx) = oneshot::channel::<()>();

    let broker_join_handle = broker_consumer_loop(
        pool.clone(),
        configuration.topic_prefix.clone(),
        Arc::new(Poster::new(&configuration.other_proxy)),
        rx,
    );

    // this will run until we get an event source error or we catch an OS signal
    let rc = event_source_loop(&configuration, pool.clone()).await;

    tracing::info!("Attempting graceful shutdown: No longer listening for events over HTTP");
    drop(tx);
    broker_join_handle.await?;

    std::process::exit(rc);
}
