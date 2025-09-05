use std::sync::Arc;

use tokio::sync::oneshot::Receiver;

pub mod amqp;
pub mod mqtt;

/// Trait which should be implemented by the application to handle a formatted message, ready to send to a server or clients
pub trait HttpBroadcast {
    /// Return true if we can consider the event to be successfully "published"
    /// note that this does not *have* to have an asynchronous internal implementation, it should just allow for one
    fn publish_event_to_http(
        &self,
        event: String,
    ) -> impl std::future::Future<Output = bool> + Send;
}

/// Trait which gets implemented by the global state, based on which protocol we will handle.
pub trait ProtoHandler {
    /// this is meant to verify errors in the message before publishing
    fn preverify_publish(&self, topic: &str) -> Result<(), String>;
    /// the assumption is that once this function is called, all faults lie in the broker (and not the parameters)
    fn publish_message(
        &self,
        topic: &str,
        data: String,
    ) -> impl std::future::Future<Output = Result<(), &str>> + Send;
    /// this should start a subscribe loop in a tokio::spawn thread, and return the JoinHandle.
    fn begin_subscribe_loop(
        &self,
        config_topic: String,
        broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
        killswitch: Receiver<()>,
    ) -> tokio::task::JoinHandle<()>;
}
