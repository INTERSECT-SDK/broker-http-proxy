use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use secrecy::ExposeSecret;

use intersect_ingress_proxy_common::intersect_messaging::extract_eventsource_data;
use intersect_ingress_proxy_common::protocols::interfaces::PublishProtoHandler;
use intersect_ingress_proxy_common::server_paths::SUBSCRIBE_URL;
use intersect_ingress_proxy_common::signals::wait_for_os_signal;

use crate::configuration::ExternalProxy;

/// Return Err only if we weren't able to publish a correct message to the broker, invalid messages are ignored
async fn send_message(
    message: String,
    proto_handler: &impl PublishProtoHandler,
) -> Result<(), &str> {
    let es_data_result = extract_eventsource_data(&message);
    if es_data_result.is_err() {
        return Ok(());
    }
    let (topic, data) = es_data_result.unwrap();

    proto_handler.publish_message(&topic, data).await
}

/// Return value - exit code to use
///
/// # Panics
///   - Inner API could potentially panic but is currently not expected to do so
pub async fn event_source_loop(
    other_proxy: ExternalProxy,
    proto_handler: impl PublishProtoHandler,
) -> i32 {
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
                                if let Err(e) = send_message(message.data, &proto_handler).await {
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
