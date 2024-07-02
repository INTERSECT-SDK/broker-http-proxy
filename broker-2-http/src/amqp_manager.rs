use amqprs::{
    channel::{
        BasicConsumeArguments, Channel, ExchangeDeclareArguments, QueueBindArguments,
        QueueDeclareArguments,
    },
    connection::Connection,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::{amqp_consumer::AmqpConsumer, broadcaster::Broadcaster, configuration::Settings};
use intersect_ingress_proxy_common::intersect_messaging::INTERSECT_MESSAGE_EXCHANGE;
use intersect_ingress_proxy_common::protocols::amqp::{get_channel, get_connection};

/// AmqpManager is meant to be a long-living object which contains connection/channel data of a consumer.
#[derive(Clone)]
pub struct AmqpManager {
    connection: Connection,
    channel: Channel,
}

impl AmqpManager {
    /// Create a new AmqpManager. We need to provide a Broadcaster because this object manages the AmqpConsumer on its own.
    pub async fn new(configuration: &Settings, broadcaster: Arc<Broadcaster>) -> Self {
        let connection = get_connection(&configuration.broker, 10).await;
        let channel = get_channel(&connection).await;

        // TODO - we probably don't need to declare this exchange, since we are always subscribing to messages on this channel.
        channel
            .exchange_declare(
                ExchangeDeclareArguments::new(INTERSECT_MESSAGE_EXCHANGE, "topic")
                    .durable(true)
                    .finish(),
            )
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

        channel
            .basic_consume(
                AmqpConsumer::new(args.no_ack, configuration.topic_prefix.clone(), broadcaster),
                args,
            )
            .await
            .unwrap();
        Self {
            connection,
            channel,
        }
    }

    /// Cleanup for the AmqpManager. The manager cannot be used after calling this.
    pub async fn destruct(self) {
        match self.channel.close().await {
            Ok(_) => tracing::debug!("closed channel"),
            Err(e) => {
                tracing::error!(error = ?e, "Could not close channel")
            }
        }
        match self.connection.close().await {
            Ok(_) => tracing::debug!("closeed connection"),
            Err(e) => {
                tracing::error!(error = ?e, "Could not close connection")
            }
        }
    }
}
