/// Make sure that the routing key matches the format expected to be used throughout the proxy app. Note that this format is expected to handle multiple protocols;
/// for example, we allow for a proxy-http-client and a proxy-http-server instance to be talking to brokers with different protocols.
///
/// Also note that we do not permit publishing on wildcards.
#[must_use]
pub fn is_routing_key_compliant(key: &str) -> bool {
    !key.chars()
        .any(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
}
