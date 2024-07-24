use amqprs::{
    channel::{BasicAckArguments, Channel},
    consumer::AsyncConsumer,
    BasicProperties, Deliver,
};
use std::sync::Arc;

use axum::async_trait;

use crate::broadcaster::Broadcaster;
use intersect_ingress_proxy_common::intersect_messaging::{
    make_eventsource_data, should_message_passthrough,
};

/// Consumer for messages. If a message is determined to be from the system,
/// the message is broadcast using the broadcaster to all appropriate channels.
pub struct AmqpConsumer {
    auto_ack: bool,
    topic_prefix: String,
    broadcaster: Arc<Broadcaster>,
}

impl AmqpConsumer {
    pub fn new(auto_ack: bool, topic_prefix: String, broadcaster: Arc<Broadcaster>) -> Self {
        Self {
            auto_ack,
            topic_prefix,
            broadcaster,
        }
    }
}

#[async_trait]
impl AsyncConsumer for AmqpConsumer {
    async fn consume(
        &mut self,
        channel: &Channel,
        deliver: Deliver,
        _basic_properties: BasicProperties,
        content: Vec<u8>,
    ) {
        // This is the major difference between our implementations and what the SDK does - we don't necessarily want to ACK (but by default we will)
        // we will always manually ACK unless nobody was available to listen to our message, in which case we should NACK and requeue.
        let mut should_ack = true;
        if deliver.redelivered() {
            tracing::warn!("message was redelivered");
        }
        tracing::debug!("consume delivery {}", deliver);
        match String::from_utf8(content) {
            Ok(utf8_data) => {
                tracing::debug!("raw message data: {}", &utf8_data);
                match should_message_passthrough(&utf8_data, &self.topic_prefix) {
                    Err(e) => {
                        tracing::error!(error = ?e, "message is valid UTF-8 but not INTERSECT JSON");
                    }
                    Ok(false) => {
                        tracing::warn!(
                            "message source is not from this system, will not broadcast it"
                        );
                    }
                    Ok(true) => {
                        let topic = deliver.routing_key();
                        let event = make_eventsource_data(topic, &utf8_data);
                        tracing::debug!("consume delivery {} , data: {}", deliver, event,);
                        // TODO handle this better later, see broadcast() documentation for details.
                        if self.broadcaster.broadcast(&event) == 0 {
                            tracing::warn!("Broadcaster did not broadcast to anybody");
                            should_ack = false;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "message data is not UTF-8, cannot be forwarded over SSE");
            }
        }

        if !self.auto_ack {
            if should_ack {
                tracing::debug!("ack to delivery {}", deliver);
                let args = BasicAckArguments::new(deliver.delivery_tag(), false);
                match channel.basic_ack(args).await {
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = ?e, "manual ack did not work"),
                };
            } else {
                // We don't acknowledge or reject the message, so we immediately get the message back.
                tracing::warn!(
                    "Some clients probably did not get delivery {}, not acknowledging the message",
                    deliver,
                );
                // TODO - if we're able to determine SPECIFIC clients who did/did not get it, we may want to explicitly reject the message.
                // match channel
                //     .basic_reject(BasicRejectArguments::new(deliver.delivery_tag(), true))
                //     .await
                // {
                //     Ok(_) => {}
                //     Err(e) => tracing::error!(error = ?e, "manual nack did not work"),
                // };
            }
        }
    }
}
