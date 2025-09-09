use amqprs::{channel::BasicPublishArguments, BasicProperties};
use deadpool_amqprs::Pool;

use crate::protocols::amqp::utils::get_channel;
use crate::protocols::proxy::is_routing_key_compliant;
use crate::{
    intersect_messaging::INTERSECT_MESSAGE_EXCHANGE, protocols::interfaces::PublishProtoHandler,
};

#[derive(Clone)]
pub struct AmqpPublishProtoHandler {
    pool: Pool,
    /// `application_name` is used for the hardcoded queue name and for debugging purposes
    application_name: &'static str,
}

impl std::fmt::Debug for AmqpPublishProtoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmqpPublishProtoHandler")
            .field("application_name", &self.application_name)
            .finish()
    }
}

impl AmqpPublishProtoHandler {
    #[must_use]
    pub fn new(pool: Pool, application_name: &'static str) -> Self {
        Self {
            pool,
            application_name,
        }
    }
}

impl PublishProtoHandler for AmqpPublishProtoHandler {
    fn preverify_publish(&self, topic: &str) -> Result<(), String> {
        if !is_routing_key_compliant(topic) {
            return Err(format!(
                "'{topic}' does not meet the INTERSECT proxy-app routing key specification."
            ));
        }
        Ok(())
    }

    async fn publish_message(&self, topic: &str, data: String) -> Result<(), &str> {
        let connection = self.pool.get().await.map_err(|e| {
            tracing::error!(error = ?e, "cannot connect to broker");
            "cannot connect to broker"
        })?;
        let channel = get_channel(&connection).await.map_err(|e| {
            tracing::error!(error = ?e, "cannot create channel on broker");
            "cannot create channel on broker"
        })?;

        let args = BasicPublishArguments::new(INTERSECT_MESSAGE_EXCHANGE, topic);
        tracing::debug!("Preparing to publish message on broker: {}", &data);
        match channel
            .basic_publish(
                BasicProperties::default().with_persistence(true).finish(),
                data.into_bytes(),
                args,
            )
            .await
        {
            Ok(()) => tracing::debug!("message published successfully"),
            Err(e) => {
                tracing::error!(error = ?e, "could not publish message");
                return Err("could not publish message");
            }
        }
        if let Err(e) = channel.close().await {
            tracing::warn!(error = ?e, "could not close channel after publishing message");
        }

        Ok(())
    }
}
