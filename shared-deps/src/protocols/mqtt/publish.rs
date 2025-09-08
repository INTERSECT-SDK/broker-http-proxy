use rumqttc::AsyncClient;

use crate::protocols::interfaces::PublishProtoHandler;

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
        Ok(())
    }
    async fn publish_message(&self, topic: &str, data: String) -> Result<(), &str> {
        Err("yippee")
    }
}
