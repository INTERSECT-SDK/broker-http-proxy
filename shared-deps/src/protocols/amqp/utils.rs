use amqprs::{
    callbacks::DefaultChannelCallback,
    channel::{Channel, ExchangeDeclareArguments, QueueBindArguments, QueueDeclareArguments},
    connection::{Connection, OpenConnectionArguments},
};
use deadpool_amqprs::{Config, Pool};
use secrecy::ExposeSecret;

use crate::{configuration::BrokerSettings, intersect_messaging::INTERSECT_MESSAGE_EXCHANGE};

/// Get an AMQP connection pool, this pool will manage all connections to the broker
///
/// NOTE: calling this function cannot fail on its own, you have to call pool.get().await for it to fail
#[must_use]
pub fn get_connection_pool(connection_details: &BrokerSettings) -> Pool {
    let mut args = OpenConnectionArguments::new(
        &connection_details.host,
        connection_details.port,
        &connection_details.username,
        connection_details.password.expose_secret(),
    );
    args.virtual_host("/");

    let config = Config::new_with_con_args(args);
    config.create_pool()
}

/// This is our initial verification step. We will make sure that we can connect and that the exchanges/queues are set up.
///
/// # Errors
///   - Errors if can't connect to server or has invalid permissions
pub async fn verify_connection_pool(pool: &Pool, queue_name_src: &str) -> Result<(), String> {
    let connection = pool
        .get()
        .await
        .map_err(|_| "Couldn't connect to broker, check your credentials.".to_string())?;

    let channel = get_channel(&connection)
        .await
        .map_err(|_| "Couldn't make verification channel")?;

    make_exchange(&channel).await.map_err(|_| {
        "Couldn't make the INTERSECT exchange or confirm that it exists.".to_string()
    })?;

    // we'll use a persistent queue named "proxy-http-client", as there should only be one proxy-http-server deployment per System
    // TODO - note that we should probably name queues larger than 127 characters with a hashed key
    let Some((queue_name, _, _)) = channel
        .queue_declare(QueueDeclareArguments::durable_client_named(queue_name_src))
        .await
        .map_err(|_| format!("Couldn't declare the {queue_name_src} queue"))?
    else {
        // unlikely this pops
        return Err("didn't get correct args back from queue declaration".to_string());
    };

    // listen for every single message on the exchange, we must do this due to the way userspace messages work
    channel
        .queue_bind(QueueBindArguments::new(
            &queue_name,
            INTERSECT_MESSAGE_EXCHANGE,
            "#",
        ))
        .await
        .map_err(|_| {
            format!(
                "Couldn't bind the {} exchange to the {} queue",
                INTERSECT_MESSAGE_EXCHANGE, &queue_name
            )
        })?;
    channel
        .close()
        .await
        .map_err(|_| "Couldn't close the setup channel".to_string())?;

    Ok(())
}

/// open a channel on the provided connection
///
/// Returns:
///   - the channel
///
/// # Errors
///   - Errors if failure to communicate with broker server
pub async fn get_channel(connection: &Connection) -> Result<Channel, amqprs::error::Error> {
    let channel = connection.open_channel(None).await?;
    channel.register_callback(DefaultChannelCallback).await?;
    Ok(channel)
}

/// logic for declaring the INTERSECT exchange - need to do this in case no services/systems have declared it
///
/// Returns:
///   - result of whether or not the exchange declaration was successful
///
/// # Errors
///   - Errors if failure to communicate with broker server
async fn make_exchange(channel: &Channel) -> Result<(), amqprs::error::Error> {
    channel
        .exchange_declare(
            ExchangeDeclareArguments::new(INTERSECT_MESSAGE_EXCHANGE, "topic")
                .durable(true)
                .finish(),
        )
        .await
}

/// make sure that the routing key is valid for AMQP
///
/// we do not permit publishing on wildcards
#[must_use]
pub(crate) fn is_routing_key_compliant(key: &str) -> bool {
    !key.chars()
        .any(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' && c != ':')
}
