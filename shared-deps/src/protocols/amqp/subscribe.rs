use amqprs::channel::{
    BasicAckArguments, BasicCancelArguments, BasicConsumeArguments, BasicRejectArguments, Channel,
    ConsumerMessage,
};

use deadpool_amqprs::Pool;
use std::sync::Arc;
use tokio::sync::oneshot::Receiver;
use uuid::Uuid;

use crate::intersect_messaging::{make_eventsource_data, should_message_passthrough};
use crate::protocols::amqp::{get_channel, verify_connection_pool};
use crate::protocols::HttpBroadcast;

pub fn broker_consumer_loop(
    amqp_connection_pool: Pool,
    config_topic: String,
    queue_name_src: &'static str,
    broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
    killswitch: Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        broker_consumer_loop_inner(
            amqp_connection_pool,
            config_topic,
            queue_name_src,
            broadcaster,
            killswitch,
        )
        .await;
    })
}

async fn broker_consumer_loop_inner(
    amqp_connection_pool: Pool,
    config_topic: String,
    queue_name_src: &str,
    broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
    // calls recv() once the EventSource loop or HTTP server catches an OS signal
    mut killswitch: Receiver<()>,
) {
    let mut needs_reverification = false;
    'connection_loop: loop {
        let connection = amqp_connection_pool.get().await;
        if connection.is_err() {
            needs_reverification = true;
            tracing::warn!("Consumer lost connection to broker, retrying in 5 seconds");
            let retry_wait = tokio::time::sleep(std::time::Duration::from_secs(5));
            tokio::pin!(retry_wait);
            tokio::select! {
                _ = &mut killswitch => {
                    // HTTP component has been shut down, no point in waiting
                    tracing::warn!("Shutting down while attempting to reconnect to broker");
                    break;
                },
                () = &mut retry_wait => {
                    continue;
                },
            }
        }

        if needs_reverification {
            if let Err(e) = verify_connection_pool(&amqp_connection_pool, queue_name_src).await {
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
        let args = BasicConsumeArguments::new(queue_name_src, &Uuid::new_v4().to_string())
            .manual_ack(true) // only ack messages we should actually publish, we will nack the others
            .finish();

        let basic_consume_rx = channel.basic_consume_rx(args).await;
        if basic_consume_rx.is_err() {
            tracing::warn!("Couldn't start consuming, trying again?");
            match channel.close().await {
                Ok(()) => tracing::debug!("closed channel"),
                Err(e) => {
                    tracing::error!(error = ?e, "Could not close channel");
                }
            }
            continue;
        }
        let (consumer_tag, mut messages_rx) = basic_consume_rx.unwrap();
        loop {
            tokio::select! {
                _ = &mut killswitch => {
                    // attempt cleanup before terminating
                    tracing::warn!("Received terminate signal from OS, attempting to gracefully disconnect from AMQP broker...");
                    cleanup(consumer_tag, channel).await;

                    break 'connection_loop;
                },
                consumer_result = messages_rx.recv() => {
                    if let Some(msg) = consumer_result {
                        consume_message(msg, &channel, &config_topic, &broadcaster, &mut killswitch).await;
                    } else {
                        tracing::warn!("Messages channel was suddenly closed, will try to reconnect");
                        break;
                    }
                }
            }
        }

        // if we reach this, the channel has been closed from the messages_rx object (most likely from a broker disconnect), so we will clean up and then attempt reconnection
        cleanup(consumer_tag, channel).await;
    }
}

/// domain logic for handling a message from the broker
async fn consume_message(
    msg: ConsumerMessage,
    channel: &Channel,
    config_topic: &str,
    broadcaster: &Arc<impl HttpBroadcast + Send + Sync + 'static>,
    killswitch: &mut Receiver<()>,
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
                    let topic = deliver.routing_key();
                    match make_eventsource_data(topic, &utf8_data) {
                        Err(_) => {}
                        Ok(event) => {
                            tracing::debug!("consume delivery {} , data: {}", deliver, event,);
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
        Err(e) => {
            // this should generally not be seen, so log as an error
            tracing::error!(error = ?e, "message data is not UTF-8, cannot be forwarded over SSE");
        }
    }

    if should_ack {
        tracing::debug!("ack to delivery {}", deliver);
        let args = BasicAckArguments::new(deliver.delivery_tag(), false);
        match channel.basic_ack(args).await {
            Ok(()) => {}
            Err(e) => tracing::error!(error = ?e, "manual ack did not work"),
        };
    } else {
        // We don't acknowledge or reject the message, so we immediately get the message back.
        tracing::warn!("Rejecting delivery {}", deliver,);
        // TODO - if we're able to determine SPECIFIC clients who did/did not get it, we may want to explicitly reject the message.
        match channel
            .basic_reject(BasicRejectArguments::new(deliver.delivery_tag(), true))
            .await
        {
            Ok(()) => {}
            Err(e) => tracing::error!(error = ?e, "manual reject did not work"),
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
    }
    match channel.close().await {
        Ok(()) => tracing::debug!("closed channel"),
        Err(e) => {
            tracing::error!(error = ?e, "Could not close channel");
        }
    }
}
