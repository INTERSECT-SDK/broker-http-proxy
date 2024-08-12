use std::sync::Arc;
use std::time::Duration;

use amqprs::{channel::BasicPublishArguments, connection::Connection, BasicProperties};
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use tokio::sync::Mutex;

use http_2_broker::configuration::Settings;
use intersect_ingress_proxy_common::configuration::get_configuration;
use intersect_ingress_proxy_common::intersect_messaging::{
    extract_eventsource_data, INTERSECT_MESSAGE_EXCHANGE,
};
use intersect_ingress_proxy_common::protocols::amqp::{
    get_channel, get_connection, is_name_compliant, make_exchange,
};
use intersect_ingress_proxy_common::signals::wait_for_os_signal;
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};
use secrecy::ExposeSecret;

/// Data we need to share across multiple closures.
struct BrokerData {
    pub connection: Mutex<Connection>,
}

async fn send_message(configuration: &Settings, message: String, broker_data: Arc<BrokerData>) {
    let es_data_result = extract_eventsource_data(&message);
    if es_data_result.is_err() {
        return;
    }
    let (topic, data) = es_data_result.unwrap();
    if !is_name_compliant(&topic) {
        tracing::warn!(
            "{} is not a valid AMQP topic name, will not attempt publish",
            topic
        );
        return;
    }

    let mut connection = broker_data.connection.lock().await;
    if !connection.is_open() {
        *connection = get_connection(&configuration.broker, 0).await;
    }

    // TODO - we'd ideally like to potentially reuse the channel instead of closing it every time
    // see https://github.com/rdoetjes/rabbit_systeminfo/blob/master/systeminfo/src/main.rs#L84 as an example
    // we NEED to explicitly close the channel, or else problems on the broker may develop
    let channel = get_channel(&connection).await;

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
                                send_message(configuration, message.data, broker_data.clone()).await;
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

    let connection = get_connection(&configuration.broker, 10).await;

    // try to declare the exchange on the broker, fail if not
    // do this outside of the hot loop, we should not try to declare the exchange on every message
    {
        let channel = get_channel(&connection).await;
        let exchange_result = make_exchange(&channel).await;
        match channel.close().await {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = ?e, "could not close channel after making exchange");
            }
        };
        if exchange_result.is_err() {
            let err = exchange_result.unwrap_err();
            tracing::error!("could not create exchange: {}", err);
            connection.close().await.expect("Could not close connection after failed exchange creation");
            std::process::exit(1);
        }
    }

    let broker_data = Arc::new(BrokerData {
        connection: Mutex::new(connection),
    });

    let rc = event_source_loop(&configuration, broker_data.clone()).await;

    tracing::info!("Attempting graceful shutdown: No longer listening for events over HTTP, will wait 3 seconds to publish remaining messages");
    tokio::time::sleep(Duration::from_secs(3)).await;
    // TODO this has a lifetime issue, can't move it out of the Arc
    // closing the connection is not quite as important, it won't cause issues on the broker and it should be auto-dropped anyways
    // match connection.close().await {
    //     Ok(_) => {}
    //     Err(err) => {
    //         tracing::warn!("Couldn't close connection");
    //     }
    // };
    tracing::info!("process gracefully shutdown");
    std::process::exit(rc);
}
