use std::sync::Arc;

use tokio::sync::oneshot::Receiver;

/// Trait which should be implemented by the application to handle a formatted message, ready to send to a server or clients
pub trait HttpBroadcast {
    /// Return true if we can consider the event to be successfully "published"
    /// note that this does not *have* to have an asynchronous internal implementation, it should just allow for one
    fn publish_event_to_http(
        &self,
        event: String,
    ) -> impl std::future::Future<Output = bool> + Send;
}

/// Trait which determines how to publish a message. Should usually show up as a reaction to receiving an HTTP event or request.
/// Note that whatever implements `PublishProtoHandler` should generally implement Clone as well.
pub trait PublishProtoHandler {
    /// this is meant to verify errors in the message before publishing
    ///
    /// # Errors
    ///   - return an error message if message verification failed.
    fn preverify_publish(&self, topic: &str) -> Result<(), String>;
    /// the assumption is that once this function is called, all faults lie in the broker (and not the parameters)
    fn publish_message(
        &self,
        topic: &str,
        data: String,
    ) -> impl std::future::Future<Output = Result<(), &str>> + Send;
}

/// Trait which determines how to subscribe to a message. Usually runs in its own thread and uses an [`HttpBroadcast`] to send the message over an HTTP channel.
pub trait SubscribeProtoHandler {
    /// this should start a subscribe loop in a [`tokio::spawn`] thread, and return the [`tokio::task::JoinHandle`] .
    /// this consumes the `SubscribeProtoHandler` itself after being called.
    fn begin_subscribe_loop(
        self,
        config_topic: String,
        broadcaster: Arc<impl HttpBroadcast + Send + Sync + 'static>,
        killswitch: Receiver<()>,
    ) -> tokio::task::JoinHandle<()>;
}
