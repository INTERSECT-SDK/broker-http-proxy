/// FOR DEVOPS USERS:
/// 1) The root struct is "Settings", follow logic from there
/// 2) integers can be provided as a string in config files or environment variables
/// 3) if using environment variables, see comment in `get_configuration()` as an example of how nesting works
/// 4) if using ONLY a file variable, this is determined from the `APP_CONFIG_FILE` environment variable (environment variables have higher precedence)
/// 5) Additional logic can be found in shared-deps/src/configuration.rs
use secrecy::SecretString;

use intersect_ingress_proxy_common::configuration::{
    deserialize_enforce_topic_prefixes, deserialize_trim_trailing_slash, BrokerSettings, LogLevel,
};

#[derive(serde::Deserialize, Clone, Debug)]
pub struct ExternalProxy {
    #[serde(deserialize_with = "deserialize_trim_trailing_slash")]
    /// URL for the other ingress proxy we are communicating with
    pub url: String,
    /// Basic authentication credentials for the other proxy
    pub username: String,
    /// Basic authentication credentials for the other proxy
    pub password: SecretString,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Settings {
    /// configuration for the broker, which our applications are listening to
    pub broker: BrokerSettings, // TODO make this a Vec<BrokerSettings>
    /// URL for the other ingress proxy we are communicating with
    pub other_proxy: ExternalProxy, // TODO make this a Vec<ExternalProxy>
    /// log level for the entire application
    pub log_level: LogLevel,
    /// set to true for developer-unfriendly settings (currently just log formats)
    pub production: bool,
    #[serde(deserialize_with = "deserialize_enforce_topic_prefixes")]
    /// this should only contain the SYSTEM prefix, i.e. "organization.facility.system."
    pub topic_prefix: String,
}
