use std::sync::Arc;
use std::time::Duration;

use amqprs::channel::ExchangeDeclareArguments;
use amqprs::{channel::BasicPublishArguments, connection::Connection, BasicProperties};
use sse_client::EventSource;

use http_2_broker::configuration::{get_configuration, Settings};
use intersect_ingress_proxy_common::intersect_messaging::{
    extract_eventsource_data, INTERSECT_MESSAGE_EXCHANGE,
};
use intersect_ingress_proxy_common::protocols::amqp::{get_channel, get_connection};
use intersect_ingress_proxy_common::signals::wait_for_os_signal;
use intersect_ingress_proxy_common::telemetry::{
    get_json_subscriber, get_pretty_subscriber, init_subscriber,
};

/// Data we need to share across multiple closures.
struct BrokerData {
    pub connection: Connection,
}

async fn send_message(message: String, broker_data: Arc<BrokerData>) {
    let (topic, data) = extract_eventsource_data(&message);

    // TODO - we'd ideally like to potentially reuse the channel instead of closing it every time
    // see https://github.com/rdoetjes/rabbit_systeminfo/blob/master/systeminfo/src/main.rs#L84 as an example
    // we NEED to explicitly close the channel, or else problems on the broker may develop
    let channel = get_channel(&broker_data.connection).await;

    // NOTE - we MUST declare this exchange, in case a message is sent to this system but no scientific services are deployed
    channel
        .exchange_declare(
            ExchangeDeclareArguments::new(INTERSECT_MESSAGE_EXCHANGE, "topic")
                .durable(true)
                .finish(),
        )
        .await
        .expect("Could not declare exchange on channel");
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

async fn event_source_loop(configuration: &Settings, broker_data: Arc<BrokerData>) {
    // TODO this is probably not the best approach, but it seems to work
    let async_bridge = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let mut es_res = EventSource::new(&configuration.other_proxy_url);
    let mut retries = 0_u32;
    while es_res.is_err() {
        if retries > 10 {
            tracing::error!("Cannot connect to event source URL, stopping application...");
            std::process::exit(1);
        }
        retries += 1;
        std::thread::sleep(Duration::from_millis(2000));
        es_res = EventSource::new(&configuration.other_proxy_url);
    }
    tracing::info!("connected");
    let event_source = es_res.unwrap();
    // TODO would be ideal for on_message to be async as well
    event_source.on_message(move |message| {
        tracing::debug!("New message event --- {:?}", message);
        async_bridge.spawn(send_message(message.data, broker_data.clone()));
    });
    event_source.add_event_listener("error", |error| {
        tracing::error!("Event source error --- {:?}", error);
        // TODO is this the best way to handle these? This usually implies a misconfiguration.
        // We may also get these if the web server goes down.
        std::process::exit(1);
    });

    wait_for_os_signal().await;
    tracing::info!("Attempting graceful shutdown...");
    tracing::info!("No longer listening for events over HTTP, will wait 3 seconds to publish remaining messages");
    event_source.close();
}

#[tokio::main]
pub async fn main() {
    let configuration = get_configuration().expect("Failed to read configuration");

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
    let broker_data = Arc::new(BrokerData { connection });

    event_source_loop(&configuration, broker_data.clone()).await;
    // TODO this has a lifetime issue, can't move it out of the Arc
    // closing the connection is not quite as important, it won't cause issues on the broker and it should be auto-dropped anyways
    // match connection.close().await {
    //     Ok(_) => {}
    //     Err(err) => {
    //         tracing::warn!("Couldn't close connection");
    //     }
    // };
    tokio::time::sleep(Duration::from_secs(3)).await;
    tracing::info!("process gracefully shutdown");
}
