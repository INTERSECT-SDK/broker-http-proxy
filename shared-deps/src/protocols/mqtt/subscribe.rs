use std::sync::Arc;

use tokio::sync::oneshot::Receiver;

use rumqttc::v5::mqttbytes::v5::Publish;
use rumqttc::v5::{AsyncClient, EventLoop};

use crate::{
    intersect_messaging::{make_eventsource_data, should_message_passthrough},
    protocols::interfaces::{HttpBroadcast, SubscribeProtoHandler},
    protocols::mqtt::utils::mqtt_topic_to_proxy_topic,
};

pub struct MqttSubscribeProtoHandler {
    mqtt_client: AsyncClient,
    mqtt_event_loop: EventLoop,
    /// `application_name` is used for the hardcoded queue name and for debugging purposes
    application_name: &'static str,
}

impl std::fmt::Debug for MqttSubscribeProtoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MqttSubscribeProtoHandler")
            .field("application_name", &self.application_name)
            .finish()
    }
}

impl MqttSubscribeProtoHandler {
    pub fn new(
        application_name: &'static str,
        mqtt_client: AsyncClient,
        mqtt_event_loop: EventLoop,
    ) -> Self {
        MqttSubscribeProtoHandler {
            mqtt_client,
            mqtt_event_loop,
            application_name,
        }
    }
}

impl SubscribeProtoHandler for MqttSubscribeProtoHandler {
    fn begin_subscribe_loop(
        self,
        config_topic: String,
        broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
        killswitch: Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            broker_consumer_loop_inner(
                self.mqtt_client,
                self.mqtt_event_loop,
                config_topic,
                broadcaster,
                killswitch,
            )
            .await;
        })
    }
}

async fn broker_consumer_loop_inner(
    mqtt_client: AsyncClient,
    mut mqtt_event_loop: EventLoop,
    config_topic: String,
    broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
    // calls recv() once the EventSource loop or HTTP server catches an OS signal
    mut killswitch: Receiver<()>,
) {
    let still_connected = loop {
        tokio::select! {
            _ = &mut killswitch => {
                break true;
            },
            event_loop_pool = mqtt_event_loop.poll() => {
                match event_loop_pool {
                    Ok(event) => {
                        match event {
                            rumqttc::v5::Event::Incoming(packet) => {
                                match packet {
                                    rumqttc::v5::mqttbytes::v5::Packet::Publish(publish_packet) => {
                                        consume_message(
                                            publish_packet,
                                            &mqtt_client,
                                            &config_topic,
                                            &broadcaster,
                                            &mut killswitch,
                                        ).await;
                                    },
                                    packet => {
                                        tracing::debug!("Incoming packet -- {packet:?}");
                                    },
                                }
                            },
                            rumqttc::v5::Event::Outgoing(outgoing) => {
                                tracing::debug!("Outgoing packet -- {outgoing:?}");
                            },
                        }
                    },
                    Err(conn_err) => {
                        tracing::warn!("Consumer lost connection to broker, retrying in 5 seconds. Specifics: {conn_err}");
                        let retry_wait = tokio::time::sleep(std::time::Duration::from_secs(5));
                        tokio::pin!(retry_wait);
                        tokio::select! {
                            _ = &mut killswitch => {
                                // HTTP component has been shut down, no point in waiting
                                tracing::warn!("Shutting down while attempting to reconnect to broker");
                                break false;
                            },
                            () = &mut retry_wait => {
                                // no-op
                            },
                        }
                    },
                }
            },
        }
    };
    if still_connected {
        let _ = mqtt_client.disconnect().await;
    }
}

async fn consume_message(
    publish_packet: Publish,
    // TODO - the client will eventually manually ACK messages
    _mqtt_client: &AsyncClient,
    config_topic: &str,
    broadcaster: &Arc<impl HttpBroadcast + Send + Sync + 'static>,
    killswitch: &mut Receiver<()>,
) {
    let mut should_ack = true;
    if publish_packet.dup {
        tracing::warn!("message was redelivered");
    }

    match String::from_utf8(publish_packet.payload.to_vec()) {
        Ok(utf8_data) => {
            tracing::debug!("got raw message data from broker: {}", &utf8_data);
            match should_message_passthrough(&utf8_data, config_topic) {
                Err(e) => {
                    // This should generally not be seen, so log it as a warning
                    tracing::warn!(error = ?e, "message is valid UTF-8 but not INTERSECT JSON");
                }
                Ok(false) => {
                    tracing::debug!(
                        "message source is not from this system, will not broadcast it"
                    );
                }
                Ok(true) => {
                    match str::from_utf8(&publish_packet.topic) {
                        Err(e) => {
                            tracing::warn!(error = ?e, "message topic is not valid UTF-8, cannot be forwarded over SSE");
                        }
                        Ok(topic) => {
                            let topic = mqtt_topic_to_proxy_topic(topic);
                            match make_eventsource_data(&topic, &utf8_data) {
                                Err(_) => {}
                                Ok(event) => {
                                    tracing::debug!(
                                        "Consume message {}, data: {}",
                                        publish_packet.pkid,
                                        event
                                    );
                                    // TODO handle this better later, see broadcast() documentation for details.
                                    tokio::select! {
                                        _ = killswitch => {
                                            // WARNING: in the client implementation, this may happen while waiting on a response, resulting in us rejecting a message we actually passed through successfully
                                            // this would only happen if we actually call publish_event_to_http(), if the killswitch was toggled before reaching here we will always do the killswitch branch.
                                            tracing::warn!("Got message from broker but did not send it over HTTP, the message will be rejected.");
                                            should_ack = false;
                                        },
                                        http_result = broadcaster.publish_event_to_http(event) => {
                                            if !http_result {
                                                tracing::warn!("Some clients may not have gotten a message, the message will be rejected.");
                                                should_ack = false;
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            // this should generally not be seen, so log as an error
            tracing::error!(error = ?e, "message data is not UTF-8, cannot be forwarded over SSE");
        }
    }

    // TODO implement acknowledgments
    if !should_ack {
        tracing::warn!(
            "We SHOULD be rejecting message {}, ACK is not implemented yet though",
            publish_packet.pkid
        );
    }
}
