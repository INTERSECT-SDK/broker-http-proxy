use secrecy::ExposeSecret;

use intersect_ingress_proxy_common::protocols::amqp::subscribe::HttpBroadcast;
use intersect_ingress_proxy_common::server_paths::PUBLISH_URL;

use crate::configuration::ExternalProxy;

pub struct Poster {
    http_client: reqwest::RequestBuilder,
}

impl Poster {
    #[must_use]
    pub fn new(proxy: &ExternalProxy) -> Self {
        let http_client = reqwest::Client::new()
            .post(format!("{}{}", proxy.url, PUBLISH_URL))
            .basic_auth(proxy.username.clone(), Some(proxy.password.expose_secret()))
            .timeout(std::time::Duration::from_secs(10));
        Self { http_client }
    }
}

impl HttpBroadcast for Poster {
    async fn publish_event_to_http(&self, event: String) -> bool {
        let result = self
            .http_client
            .try_clone()
            .expect("This message body shouldn't be a stream but somehow is")
            .body(event)
            .send()
            .await;
        if result.is_ok() {
            let result = result.unwrap();
            let status = result.status().as_u16();
            match result.bytes().await {
                Ok(bytes) => tracing::debug!("{:?}", bytes),
                Err(err) => tracing::debug!("ERROR: {}", err.to_string()),
            }
            status < 400
        } else {
            let result = result.unwrap_err();
            tracing::error!("response is error {}", result.to_string());
            false
        }
    }
}
