use std::sync::Arc;

use amqprs::{channel::BasicPublishArguments, BasicProperties};
use deadpool_amqprs::Pool;
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
//use tokio::sync::Barrier;

use http_2_broker::configuration::Settings;
use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::intersect_messaging::{
    extract_eventsource_data, INTERSECT_MESSAGE_EXCHANGE,
};
use intersect_ingress_proxy_common::protocols::amqp::{
    get_channel, get_connection_pool, is_routing_key_compliant, verify_connection_pool,
};
use intersect_ingress_proxy_common::signals::wait_for_os_signal;
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};
use secrecy::ExposeSecret;

/// Data we need to share across multiple closures.
struct BrokerData {
    pub amqp_connection_pool: Pool,
}

async fn send_message(message: String, broker_data: Arc<BrokerData>) {
    let es_data_result = extract_eventsource_data(&message);
    if es_data_result.is_err() {
        return;
    }
    let (topic, data) = es_data_result.unwrap();
    if !is_routing_key_compliant(&topic) {
        tracing::warn!(
            "{} is not a valid AMQP topic name, will not attempt publish",
            topic
        );
        return;
    }
    tracing::debug!("Publishing message with topic: {}", &topic);

    let connection = broker_data.amqp_connection_pool.get().await.unwrap();

    let channel = get_channel(&connection).await.unwrap();

    let args = BasicPublishArguments::new(INTERSECT_MESSAGE_EXCHANGE, &topic);
    // NOTE: the publish() function takes ownership of the string, if you don't care about logging then don't clone
    match channel
        .basic_publish(
            BasicProperties::default().with_persistence(true).finish(),
            data.clone().into_bytes(),
            args,
        )
        .await
    {
        Ok(_) => tracing::debug!("message published successfully: {}", data),
        Err(e) => {
            tracing::error!(error = ?e, "could not publish message: {}", data);
        }
    };
    match channel.close().await {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = ?e, "could not close channel");
        }
    };
}

/// Return value - exit code to use
async fn event_source_loop(configuration: &Settings, broker_data: Arc<BrokerData>) -> i32 {
    let mut es = EventSource::new(
        reqwest::Client::new()
            .get(&configuration.other_proxy.url)
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
                                send_message(message.data, broker_data.clone()).await;
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
pub async fn main() {
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

    //let barrier = Arc::new(Barrier::new(2));
    let broker_data = Arc::new(BrokerData {
        amqp_connection_pool: pool.clone(),
    });

    let rc = event_source_loop(&configuration, broker_data.clone()).await;

    tracing::info!("Attempting graceful shutdown: No longer listening for events over HTTP");
    // barrier.wait().await;
    tracing::info!("process gracefully shutdown");
    std::process::exit(rc);
}
