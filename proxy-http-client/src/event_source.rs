use deadpool_amqprs::Pool;
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use secrecy::ExposeSecret;

use intersect_ingress_proxy_common::intersect_messaging::extract_eventsource_data;
use intersect_ingress_proxy_common::protocols::amqp::{
    get_channel, is_routing_key_compliant, publish::amqp_publish_message,
};
use intersect_ingress_proxy_common::server_paths::SUBSCRIBE_URL;
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

use crate::configuration::ExternalProxy;

/// Return Err only if we weren't able to publish a correct message to the broker, invalid messages are ignored
async fn send_message(message: String, connection_pool: &Pool) -> Result<(), String> {
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
        Ok(()) => Ok(()),
        Err(()) => Err(
            "WARNING: message received from other proxy was NOT published on our own broker."
                .into(),
        ),
    }
}

/// Return value - exit code to use
///
/// # Panics
///   - Inner API could potentially panic but is currently not expected to do so
pub async fn event_source_loop(other_proxy: ExternalProxy, connection_pool: Pool) -> i32 {
    let mut es = EventSource::new(
        reqwest::Client::new()
            .get(format!("{}{}", &other_proxy.url, SUBSCRIBE_URL))
            .basic_auth(
                &other_proxy.username,
                Some(&other_proxy.password.expose_secret()),
            ),
    )
    .expect("The event source request body was somehow a stream?");
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
                                tracing::info!("connected to {}", &other_proxy.url);
                            },
                            Ok(Event::Message(message)) => {
                                if let Err(e) = send_message(message.data, &connection_pool).await {
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
            () = wait_for_os_signal() => {
                break;
            },
        };
    }
    es.close();

    rc
}
