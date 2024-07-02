use amqprs::{
    channel::{BasicAckArguments, /*BasicNackArguments,*/ Channel},
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
        // This is the major difference between our implementations and what the SDK does - we don't necessarily want to ACK
        // this is because we will be seeing messages we DON'T want to "pass through", and thus we have no business consuming them ourselves.
        let mut should_ack = false;
        if deliver.redelivered() {
            tracing::warn!("message was redelivered");
        }
        tracing::debug!("consume delivery {} on channel {}", deliver, channel,);
        match String::from_utf8(content) {
            Ok(utf8_data) => match should_message_passthrough(&utf8_data, &self.topic_prefix) {
                Err(e) => {
                    tracing::error!(error = ?e, "message is valid UTF-8 but not INTERSECT JSON");
                }
                Ok(false) => {
                    tracing::warn!(
                        "got probable duplicate message on channel {}, ignoring",
                        channel
                    );
                }
                Ok(true) => {
                    let topic = deliver.routing_key();
                    let event = make_eventsource_data(topic, &utf8_data);
                    tracing::debug!(
                        "consume delivery {} on channel {}, data: {}",
                        deliver,
                        channel,
                        event,
                    );
                    // TODO handle this better later, see broadcast() documentation for details.
                    if self.broadcaster.broadcast(&event) > 0 {
                        should_ack = true;
                    } else {
                        tracing::warn!("Broadcaster did not broadcast to anybody");
                    }
                }
            },
            Err(e) => {
                tracing::error!(error = ?e, "message data is not UTF-8, cannot be forwarded over SSE");
            }
        }

        if !self.auto_ack {
            if should_ack {
                tracing::debug!("ack to delivery {} on channel {}", deliver, channel);
                let args = BasicAckArguments::new(deliver.delivery_tag(), false);
                match channel.basic_ack(args).await {
                    Ok(_) => {}
                    Err(e) => tracing::error!(error = ?e, "manual ack did not work"),
                };
            } else {
                // TODO determine ack/nack logic later
                tracing::warn!(
                    "Some clients probably did not get delivery {} on channel {}",
                    deliver,
                    channel
                );
                //     tracing::debug!("nack to delivery {} on channel {}", deliver, channel);
                //     // TODO should we nack here or just do nothing?
                //     let args = BasicNackArguments::new(deliver.delivery_tag(), false, false);
                //     match channel.basic_nack(args).await {
                //         Ok(_) => {}
                //         Err(e) => tracing::error!(error = ?e, "manual nack did not work"),
                //     };
            }
        }
    }
}
