use amqprs::{
    callbacks::{DefaultChannelCallback, DefaultConnectionCallback},
    channel::{Channel, ExchangeDeclareArguments, QueueBindArguments, QueueDeclareArguments},
    connection::{Connection, OpenConnectionArguments},
};
use deadpool_amqprs::{Config, Pool};
use secrecy::ExposeSecret;
use std::time::Duration;

use crate::{configuration::BrokerSettings, intersect_messaging::INTERSECT_MESSAGE_EXCHANGE};

pub mod publish;

pub const APPLICATION_QUEUE_NAME: &str = "http-2-broker";

/// Get an AMQP connection pool, this pool will manage all connections to the broker
///
/// NOTE: calling this function cannot fail on its own, you have to call pool.get().await for it to fail
///
/// usually, you can just wait for it to fail in the subscriber
pub async fn get_connection_pool(connection_details: &BrokerSettings) -> Pool {
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
pub async fn verify_connection_pool(pool: &Pool) -> Result<(), String> {
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

    // we'll use a persistent queue named "http-2-broker", as there should only be one broker-2-http deployment per System
    // TODO - note that we should probably name queues larger than 127 characters with a hashed key
    let (queue_name, _, _) = channel
        .queue_declare(QueueDeclareArguments::durable_client_named(
            APPLICATION_QUEUE_NAME,
        ))
        .await
        .map_err(|_| format!("Couldn't declare the {} queue", APPLICATION_QUEUE_NAME))?
        .expect("didn't get correct args back from queue declaration"); // unlikely this pops

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
                INTERSECT_MESSAGE_EXCHANGE, APPLICATION_QUEUE_NAME
            )
        })?;
    channel
        .close()
        .await
        .map_err(|_| "Couldn't close the setup channel".to_string())?;

    Ok(())
}

/// Connect to the broker, attempt to reconnect if failed initially.
/// if retries = 0, retry forever
///
/// Returns:
///   - the connection
pub async fn get_connection(connection_details: &BrokerSettings, retries: u32) -> Connection {
    let mut res = Connection::open(
        OpenConnectionArguments::new(
            &connection_details.host,
            connection_details.port,
            &connection_details.username,
            connection_details.password.expose_secret(),
        )
        .virtual_host("/"),
    )
    .await;

    let mut attempts = 0;
    while res.is_err() {
        if retries != 0 {
            attempts += 1;
            if attempts > retries {
                tracing::error!("Too many failed connections, killing application");
                std::process::exit(1);
            }
        }
        tracing::error!("trying to connect after error");
        tokio::time::sleep(Duration::from_millis(2000)).await;
        res = Connection::open(&OpenConnectionArguments::new(
            &connection_details.host,
            connection_details.port,
            &connection_details.username,
            connection_details.password.expose_secret(),
        ))
        .await;
    }

    let connection = res.unwrap();
    connection
        .register_callback(DefaultConnectionCallback)
        .await
        .unwrap();
    connection
}

/// open a channel on the provided connection
///
/// Returns:
///   - the channel
pub async fn get_channel(connection: &Connection) -> Result<Channel, amqprs::error::Error> {
    let channel = connection.open_channel(None).await?;
    channel.register_callback(DefaultChannelCallback).await?;
    Ok(channel)
}

/// logic for declaring the INTERSECT exchange - need to do this in case no services/systems have declared it
///
/// Returns:
///   - result of whether or not the exchange declaration was successful
pub async fn make_exchange(channel: &Channel) -> Result<(), amqprs::error::Error> {
    channel
        .exchange_declare(
            ExchangeDeclareArguments::new(INTERSECT_MESSAGE_EXCHANGE, "topic")
                .durable(true)
                .finish(),
        )
        .await
}

/// make sure that the routing key is valid for AMQP
/// we do not permit publishing on wildcards
pub fn is_routing_key_compliant(key: &str) -> bool {
    !key.chars()
        .any(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' && c != ':')
}
