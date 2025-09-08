use rumqttc::{AsyncClient, QoS};

use crate::protocols::interfaces::PublishProtoHandler;
use crate::protocols::proxy::is_routing_key_compliant;

#[derive(Clone)]
pub struct MqttPublishProtoHandler {
    mqtt_client: AsyncClient,
    /// application_name is used for the hardcoded queue name and for debugging purposes
    application_name: &'static str,
}

impl std::fmt::Debug for MqttPublishProtoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MqttPublishProtoHandler")
            .field("application_name", &self.application_name)
            .finish()
    }
}

impl MqttPublishProtoHandler {
    pub fn new(application_name: &'static str, mqtt_client: AsyncClient) -> Self {
        MqttPublishProtoHandler {
            mqtt_client,
            application_name,
        }
    }
}

impl PublishProtoHandler for MqttPublishProtoHandler {
    fn preverify_publish(&self, topic: &str) -> Result<(), String> {
        if !is_routing_key_compliant(topic) {
            return Err(format!(
                "'{topic}' does not meet the INTERSECT proxy-app routing key specification."
            ));
        }
        Ok(())
    }
    async fn publish_message(&self, topic: &str, data: String) -> Result<(), &str> {
        self.mqtt_client
            .publish(topic, QoS::AtLeastOnce, true, data.into_bytes())
            .await
            .map_err(|err| {
                tracing::error!("Could not publish message -- {err}");
                "could not publish message"
            })
    }
}
