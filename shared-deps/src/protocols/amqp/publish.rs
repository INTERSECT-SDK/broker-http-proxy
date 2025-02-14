use amqprs::{
    channel::{BasicPublishArguments, Channel},
    BasicProperties,
};

use crate::intersect_messaging::INTERSECT_MESSAGE_EXCHANGE;

/// publish an INTERSECT message on the broker
/// if unable to publish, return an error
pub async fn amqp_publish_message(channel: Channel, topic: &str, data: String) -> Result<(), ()> {
    let args = BasicPublishArguments::new(INTERSECT_MESSAGE_EXCHANGE, topic);
    // NOTE: the publish() function takes ownership of the string, if you don't care about logging then don't clone
    match channel
        .basic_publish(
            BasicProperties::default().with_persistence(true).finish(),
            data.clone().into_bytes(),
            args,
        )
        .await
    {
        Ok(_) => tracing::debug!("message published successfully: {}", data),
        Err(e) => {
            tracing::error!(error = ?e, "could not publish message: {}", data);
            return Err(());
        }
    };
    match channel.close().await {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = ?e, "could not close channel after publishing message");
        }
    };
    Ok(())
}
