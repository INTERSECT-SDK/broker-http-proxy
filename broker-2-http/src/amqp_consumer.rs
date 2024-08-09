use amqprs::{
    channel::{
        BasicAckArguments, BasicCancelArguments, BasicConsumeArguments, Channel, ConsumerMessage,
        QueueBindArguments, QueueDeclareArguments,
    },
    connection::Connection,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::broadcaster::Broadcaster;
use intersect_ingress_proxy_common::intersect_messaging::INTERSECT_MESSAGE_EXCHANGE;
use intersect_ingress_proxy_common::protocols::amqp::{get_channel, get_connection, make_exchange};
use intersect_ingress_proxy_common::{
    configuration::BrokerSettings,
    intersect_messaging::{make_eventsource_data, should_message_passthrough},
    signals::wait_for_os_signal,
};

pub async fn broker_consumer_loop(
    config_broker: BrokerSettings,
    config_topic: String,
    broadcaster: Arc<Broadcaster>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        broker_consumer_loop_inner(config_broker, config_topic, broadcaster).await
    })
}

async fn broker_consumer_loop_inner(
    config_broker: BrokerSettings,
    config_topic: String,
    broadcaster: Arc<Broadcaster>,
) {
    let mut connected_once = false;

    'connection_loop: loop {
        let connection = get_connection(&config_broker, if connected_once { 0 } else { 10 }).await;
        let channel = get_channel(&connection).await;
        connected_once = true;

        make_exchange(&channel)
            .await
            .expect("Could not declare exchange on channel");

        // we'll use a persistent queue named "broker-2-http", as there should only be one broker-2-http deployment per System
        // TODO - note that we should probably name queues larger than 127 characters with a hashed key
        let (queue_name, _, _) = channel
            .queue_declare(QueueDeclareArguments::durable_client_named("broker-2-http"))
            .await
            .expect("Couldn't declare queue")
            .expect("didn't get correct args back from queue declaration");

        // listen for every single message on the exchange, we must do this due to the way userspace messages work
        channel
            .queue_bind(QueueBindArguments::new(
                &queue_name,
                INTERSECT_MESSAGE_EXCHANGE,
                "#",
            ))
            .await
            .expect("Couldn't bind to queue");

        // Do NOT automatically acknowledge messages, we may not be able to forward them.
        let args = BasicConsumeArguments::new(&queue_name, &Uuid::new_v4().to_string())
            .manual_ack(true) // only ack messages we should actually publish, we will nack the others
            .finish();

        let (consumer_tag, mut messages_rx) = channel.basic_consume_rx(args).await.unwrap();
        loop {
            tokio::select! {
                // OS kill signal
                _ = wait_for_os_signal() => {
                    // attempt cleanup before terminating
                    tracing::warn!("Received terminate signal from OS, attempting to gracefully disconnect from AMQP broker...");
                    cleanup(consumer_tag, channel, connection).await;

                    break 'connection_loop;
                },
                consumer_result = messages_rx.recv() => {
                    match consumer_result {
                        Some(msg) => consume_message(msg, &channel, &config_topic, broadcaster.clone()).await,
                        None => {
                            tracing::warn!("Messages channel was suddenly closed, will try to reconnect");
                            break;
                        },
                    }
                }
            }
        }

        // if we reach this, the channel has been closed from the messages_rx object (most likely from a broker disconnect), so we will clean up and then attempt reconnection
        cleanup(consumer_tag, channel, connection).await;
    }
}

/// domain logic for handling a message from the broker
async fn consume_message(
    msg: ConsumerMessage,
    channel: &Channel,
    config_topic: &str,
    broadcaster: Arc<Broadcaster>,
) {
    let deliver = msg.deliver.unwrap();
    let content = msg.content.unwrap();

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
            match should_message_passthrough(&utf8_data, config_topic) {
                Err(e) => {
                    tracing::error!(error = ?e, "message is valid UTF-8 but not INTERSECT JSON");
                }
                Ok(false) => {
                    tracing::warn!("message source is not from this system, will not broadcast it");
                }
                Ok(true) => {
                    let topic = deliver.routing_key();
                    let event = make_eventsource_data(topic, &utf8_data);
                    tracing::debug!("consume delivery {} , data: {}", deliver, event,);
                    // TODO handle this better later, see broadcast() documentation for details.
                    if broadcaster.broadcast(&event) == 0 {
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

/// call this if we were instructed to shut down or our channel suddenly disconnected.
async fn cleanup(consumer_tag: String, channel: Channel, connection: Connection) {
    if let Err(e) = channel
        .basic_cancel(BasicCancelArguments::new(&consumer_tag))
        .await
    {
        tracing::error!(error = ?e, "could not send cancel message");
    };
    match channel.close().await {
        Ok(_) => tracing::debug!("closed channel"),
        Err(e) => {
            tracing::error!(error = ?e, "Could not close channel")
        }
    }
    match connection.close().await {
        Ok(_) => tracing::debug!("closeed connection"),
        Err(e) => {
            tracing::error!(error = ?e, "Could not close connection")
        }
    }
}
