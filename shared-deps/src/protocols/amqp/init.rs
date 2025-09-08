use crate::{
    configuration::BrokerSettings,
    protocols::amqp::{
        publish::AmqpPublishProtoHandler,
        subscribe::AmqpSubscribeProtoHandler,
        utils::{get_connection_pool, verify_connection_pool},
    },
};

/// Sets up the AMQP proto handlers, and verifies that we can connect to the AMQP broker.
///
/// # Errors
///   - If we can't make an initial connection to the broker, return Err.
pub async fn init_amqp_proto_handlers(
    broker_config: &BrokerSettings,
    application_name: &'static str,
) -> Result<(AmqpPublishProtoHandler, AmqpSubscribeProtoHandler), String> {
    let pool = get_connection_pool(broker_config);
    verify_connection_pool(&pool, application_name).await?;

    let publish_handler = AmqpPublishProtoHandler::new(pool.clone(), application_name);
    let subscribe_handler = AmqpSubscribeProtoHandler::new(pool, application_name);
    Ok((publish_handler, subscribe_handler))
}
