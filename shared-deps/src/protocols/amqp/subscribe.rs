use amqprs::channel::{
    BasicAckArguments, BasicCancelArguments, BasicConsumeArguments, BasicRejectArguments, Channel,
    ConsumerMessage,
};
use deadpool_amqprs::Pool;
use std::sync::Arc;
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::protocols::amqp::{get_channel, verify_connection_pool, APPLICATION_QUEUE_NAME};
use crate::{
    intersect_messaging::{make_eventsource_data, should_message_passthrough},
    signals::wait_for_os_signal,
};

pub trait Broadcast {
    /// Return true if we can consider the event to be successfully "published"
    fn publish_event(&self, event: &str) -> bool;
}

pub async fn broker_consumer_loop(
    amqp_connection_pool: Pool,
    config_topic: String,
    broadcaster: Arc<impl Broadcast + Send + Sync + 'static>,
    barrier: Arc<Barrier>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        broker_consumer_loop_inner(amqp_connection_pool, config_topic, broadcaster, barrier).await
    })
}

async fn broker_consumer_loop_inner(
    amqp_connection_pool: Pool,
    config_topic: String,
    broadcaster: Arc<impl Broadcast>,
    barrier: Arc<Barrier>,
) {
    let mut needs_reverification = false;
    'connection_loop: loop {
        let connection = amqp_connection_pool.get().await;
        if connection.is_err() {
            needs_reverification = true;
            tracing::warn!("Consumer lost connection to broker, retrying in 5 seconds");
            let future = tokio::time::sleep(std::time::Duration::from_secs(5));
            tokio::pin!(future);
            tokio::select! {
                _ = wait_for_os_signal() => {
                    break;
                },
                _ = &mut future => {
                    continue;
                },
            }
        }

        if needs_reverification {
            if let Err(e) = verify_connection_pool(&amqp_connection_pool).await {
                tracing::warn!(error = ?e, "Couldn't fully recover broker setup");
                continue;
            }
        }
        needs_reverification = true;

        let channel_result = get_channel(&connection.unwrap()).await;
        if let Err(e) = channel_result {
            tracing::warn!(error = ?e, "Couldn't get channel, trying again?");
            continue;
        }
        let channel = channel_result.unwrap();

        // Do NOT automatically acknowledge messages, we may not be able to forward them.
        let args = BasicConsumeArguments::new(APPLICATION_QUEUE_NAME, &Uuid::new_v4().to_string())
            .manual_ack(true) // only ack messages we should actually publish, we will nack the others
            .finish();

        let consume_result = channel.basic_consume_rx(args).await;
        if consume_result.is_err() {
            tracing::warn!("Couldn't start consuming, trying again?");
            match channel.close().await {
                Ok(_) => tracing::debug!("closed channel"),
                Err(e) => {
                    tracing::error!(error = ?e, "Could not close channel")
                }
            }
            continue;
        }
        let (consumer_tag, mut messages_rx) = consume_result.unwrap();
        loop {
            tokio::select! {
                // OS kill signal
                _ = wait_for_os_signal() => {
                    // attempt cleanup before terminating
                    tracing::warn!("Received terminate signal from OS, attempting to gracefully disconnect from AMQP broker...");
                    cleanup(consumer_tag, channel).await;

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
        cleanup(consumer_tag, channel).await;
    }
    barrier.wait().await;
}

/// domain logic for handling a message from the broker
async fn consume_message(
    msg: ConsumerMessage,
    channel: &Channel,
    config_topic: &str,
    broadcaster: Arc<impl Broadcast>,
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
                    if !broadcaster.publish_event(&event) {
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
        match channel
            .basic_reject(BasicRejectArguments::new(deliver.delivery_tag(), true))
            .await
        {
            Ok(_) => {}
            Err(e) => tracing::error!(error = ?e, "manual nack did not work"),
        };
    }
}

/// call this if we were instructed to shut down or our channel suddenly disconnected.
async fn cleanup(consumer_tag: String, channel: Channel) {
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
}
