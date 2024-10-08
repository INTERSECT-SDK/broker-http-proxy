use amqprs::{
    callbacks::{DefaultChannelCallback, DefaultConnectionCallback},
    channel::{Channel, ExchangeDeclareArguments},
    connection::{Connection, OpenConnectionArguments},
};
use secrecy::ExposeSecret;
use std::time::Duration;

use crate::{configuration::BrokerSettings, intersect_messaging::INTERSECT_MESSAGE_EXCHANGE};

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
pub async fn get_channel(connection: &Connection) -> Channel {
    let channel = connection.open_channel(None).await.unwrap();
    channel
        .register_callback(DefaultChannelCallback)
        .await
        .unwrap();
    channel
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

/// make sure that "name" is a valid AMQP exchange or queue name (for publishing on)
pub fn is_name_compliant(name: &str) -> bool {
    name.len() < 128
        && !name
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' && c != ':')
}

/// make sure that the routing key is valid for AMQP
pub fn is_routing_key_compliant(key: &str) -> bool {
    key.len() < 256
}
