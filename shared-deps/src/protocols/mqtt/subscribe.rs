use std::sync::Arc;

use tokio::sync::oneshot::Receiver;

use rumqttc::{AsyncClient, EventLoop};

use crate::protocols::interfaces::{HttpBroadcast, SubscribeProtoHandler};

pub struct MqttSubscribeProtoHandler {
    mqtt_client: AsyncClient,
    mqtt_event_loop: EventLoop,
    /// application_name is used for the hardcoded queue name and for debugging purposes
    application_name: &'static str,
}

impl std::fmt::Debug for MqttSubscribeProtoHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MqttSubscribeProtoHandler")
            .field("application_name", &self.application_name)
            .finish()
    }
}

impl MqttSubscribeProtoHandler {
    pub fn new(
        application_name: &'static str,
        mqtt_client: AsyncClient,
        mqtt_event_loop: EventLoop,
    ) -> Self {
        MqttSubscribeProtoHandler {
            mqtt_client,
            mqtt_event_loop,
            application_name,
        }
    }
}

impl SubscribeProtoHandler for MqttSubscribeProtoHandler {
    fn begin_subscribe_loop(
        self,
        config_topic: String,
        broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
        killswitch: Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            broker_consumer_loop_inner(
                self.mqtt_client,
                self.mqtt_event_loop,
                config_topic,
                self.application_name,
                broadcaster,
                killswitch,
            )
            .await;
        })
    }
}

async fn broker_consumer_loop_inner(
    mqtt_client: AsyncClient,
    mqtt_event_loop: EventLoop,
    config_topic: String,
    queue_name_src: &str,
    broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
    // calls recv() once the EventSource loop or HTTP server catches an OS signal
    mut killswitch: Receiver<()>,
) {
    let mut needs_reverification = false;
    'connection_loop: loop {}
}
