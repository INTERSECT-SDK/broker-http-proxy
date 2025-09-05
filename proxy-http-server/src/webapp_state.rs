use std::sync::Arc;

use secrecy::SecretString;

use intersect_ingress_proxy_common::protocols::{
    amqp::AmqpProtoHandler, mqtt::MqttProtoHandler, ProtoHandler,
};

use crate::broadcaster::Broadcaster;

/// This is state that can be accessed by any endpoint on the server.
pub trait WebApplicationState {
    fn get_proto_handler(&self) -> &impl ProtoHandler;
    fn get_broadcaster(&self) -> &Arc<Broadcaster>;
    fn get_username(&self) -> &str;
    fn get_password(&self) -> &SecretString;
}

/// AMQP specific state
#[derive(Clone)]
pub struct AmqpWebApplicationState {
    /// protocol handler which can publish to the message broker
    pub proto_handler: AmqpProtoHandler,
    /// this broadcaster gets messages published to it from one source and can publish many messages from it. Use this if an HTTP endpoint needs to react to a subscription.
    pub broadcaster: Arc<Broadcaster>,
    /// basic auth username
    pub username: String,
    /// basic auth password
    pub password: SecretString,
}

impl WebApplicationState for AmqpWebApplicationState {
    fn get_proto_handler(&self) -> &impl ProtoHandler {
        &self.proto_handler
    }

    fn get_broadcaster(&self) -> &Arc<Broadcaster> {
        &self.broadcaster
    }

    fn get_username(&self) -> &str {
        &self.username
    }

    fn get_password(&self) -> &SecretString {
        &self.password
    }
}

/// MQTT specific state
pub struct MqttWebApplicationState {
    /// protocol handler which can publish to the message broker
    pub proto_handler: MqttProtoHandler,
    /// this broadcaster gets messages published to it from one source and can publish many messages from it. Use this if an HTTP endpoint needs to react to a subscription.
    pub broadcaster: Arc<Broadcaster>,
    /// basic auth username
    pub username: String,
    /// basic auth password
    pub password: SecretString,
}

impl WebApplicationState for MqttWebApplicationState {
    fn get_proto_handler(&self) -> &impl ProtoHandler {
        &self.proto_handler
    }

    fn get_broadcaster(&self) -> &Arc<Broadcaster> {
        &self.broadcaster
    }

    fn get_username(&self) -> &str {
        &self.username
    }

    fn get_password(&self) -> &SecretString {
        &self.password
    }
}
