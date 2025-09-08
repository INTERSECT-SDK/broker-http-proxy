use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions};
use secrecy::ExposeSecret;

use crate::{
    configuration::BrokerSettings,
    protocols::mqtt::{
        publish::MqttPublishProtoHandler, subscribe::MqttSubscribeProtoHandler,
        utils::subscribe_all,
    },
};

/// Sets up the MQTT proto handlers, and verifies that we can connect to the MQTT broker.
pub async fn init_mqtt_proto_handlers(
    broker_config: &BrokerSettings,
    application_name: &'static str,
) -> Result<(MqttPublishProtoHandler, MqttSubscribeProtoHandler), String> {
    let mut mqtt_options = MqttOptions::new(
        application_name,
        broker_config.host.clone(),
        broker_config.port,
    );
    mqtt_options.set_credentials(
        broker_config.username.clone(),
        broker_config.password.expose_secret(),
    );
    // TODO may want to handle message sizes >= 4 GiB
    mqtt_options.set_max_packet_size(1 << 32, 1 << 32);
    mqtt_options.set_clean_session(false);
    // TODO - implement manual acks
    // mqtt_options.set_manual_acks(true);
    mqtt_options.set_keep_alive(Duration::from_secs(60));

    let (mqtt_client, mut mqtt_event_loop) = AsyncClient::new(mqtt_options, 256);

    // verify initial connection
    match mqtt_event_loop.poll().await {
        Ok(event) => {
            match event {
                rumqttc::Event::Incoming(packet) => {
                    match packet {
                        rumqttc::Packet::ConnAck(conn_ack) => {
                            // expected
                            tracing::info!("Connected -- {conn_ack:?}");
                        }
                        p => {
                            // unexpected incoming event, however it shouldn't be fatal
                            tracing::warn!("unexpected initial incoming event -- {p:?}");
                        }
                    }
                }
                rumqttc::Event::Outgoing(outgoing) => {
                    // all of these are unexpected, this should probably not be seen but shouldn't be fatal
                    tracing::warn!("unexpected initial outgoing event -- {outgoing:?}");
                }
            }
        }
        Err(err) => {
            // should be fatal
            return Err(err.to_string());
        }
    }

    // listen for every single message on the exchange, we must do this due to the way userspace messages work
    subscribe_all(&mqtt_client).await?;

    let publish_handler = MqttPublishProtoHandler::new(application_name, mqtt_client.clone());
    let subscribe_handler =
        MqttSubscribeProtoHandler::new(application_name, mqtt_client, mqtt_event_loop);

    Ok((publish_handler, subscribe_handler))
}
