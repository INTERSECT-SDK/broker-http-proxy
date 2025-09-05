#![allow(unused_variables)]

use std::sync::Arc;

use tokio::sync::oneshot::Receiver;

use crate::{
    configuration::BrokerSettings,
    protocols::{HttpBroadcast, ProtoHandler},
};

#[derive(Clone)]
pub struct MqttProtoHandler {
    /// application_name is used for the hardcoded queue name and for debugging purposes
    application_name: &'static str,
}

impl std::fmt::Debug for MqttProtoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AmqpProtoHandler")
            .field("application_name", &self.application_name)
            .finish()
    }
}

impl MqttProtoHandler {
    pub async fn new(
        config: &BrokerSettings,
        application_name: &'static str,
    ) -> Result<Self, String> {
        // TODO
        Ok(MqttProtoHandler { application_name })
    }
}

impl ProtoHandler for MqttProtoHandler {
    fn preverify_publish(&self, topic: &str) -> Result<(), String> {
        todo!()
    }
    async fn publish_message(&self, topic: &str, data: String) -> Result<(), &str> {
        todo!()
    }
    fn begin_subscribe_loop(
        &self,
        config_topic: String,
        broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
        killswitch: Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        todo!()
    }
}
