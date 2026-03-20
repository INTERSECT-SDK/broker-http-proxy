use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::AsyncClient;

pub(crate) async fn subscribe_all(mqtt_client: &AsyncClient) -> Result<(), String> {
    mqtt_client
        .subscribe("#", QoS::AtLeastOnce)
        .await
        .map_err(|e| e.to_string())
}

/// convert an MQTT topic representation to the proxy's expected topic representation (which mirrors AMQP)
///
/// this should generally happen at some point in the subscribe loop
pub(crate) fn mqtt_topic_to_proxy_topic(topic: &str) -> String {
    topic.replace('/', ".")
}
