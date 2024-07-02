use axum::response::sse::Event;
use std::sync::Arc;
use tokio::sync::broadcast;

/// This broadcaster is an optimized implementation of a single-producer, multi-consumer channel.
/// The Broadcaster is effectively the "link" between the broker and the HTTP gateway.
/// If the broker decides to broadcast data, all SSE clients will asynchronosly receive it.
pub struct Broadcaster {
    fanout: broadcast::Sender<Event>,
    // TODO - this variable only makes sense once BOTH of the following are true:
    // 1) We have authentication/authorization set up (so randoms can't subscribe)
    // 2) We have multiple expected SSE clients. With just one we don't need this.
    //expected_client_count: usize,
}

impl Broadcaster {
    /// Create the broadcaster. Note that it automatically wraps it in an Arc.
    /// The broadcaster manages its producer but does not manage its consumers
    pub fn new() -> Arc<Self> {
        // TODO may be able to get away with smaller capacity
        let (tx, _) = broadcast::channel(16);
        Arc::new(Broadcaster { fanout: tx })
    }

    /// Add a broadcaster consumer - the calling function is responsible for cleaning up the consumer
    pub fn add_client(&self) -> broadcast::Receiver<Event> {
        self.fanout.subscribe()
    }

    /// Produce a message to be broadcast to all consumers. We handle the string -> SSE Event conversion here
    /// so the consumer logic is minimal.
    ///
    /// This function returns the number of clients who got the message.
    ///
    /// TODO - once we have multiple SSE clients, we may want to determine who missed out on what message.
    /// This probably involves:
    /// 1) getting SSE client information so we know who DID get it, then deduce who didn't from the config list
    /// 2) for each client who DIDN'T, NACK the message on a special exchange (dedicated to these clients).
    /// 3) Somehow transfer these messages over to the other message broker, make the messages their responsibility.
    /// Once the messages are on the other message broker, broker-2-http and http-2-broker don't need to care, handling them will be the SDK's job.
    pub fn broadcast(&self, event: &str) -> usize {
        self.fanout.send(Event::default().data(event)).unwrap_or(0)
    }
}
