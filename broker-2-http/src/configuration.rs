/// FOR DEVOPS USERS:
/// 1) The root struct is "Settings", follow logic from there
/// 2) integers can be provided as a string in config files or environment variables
/// 3) if using environment variables, see comment in "get_configuration()" as an example of how nesting works
/// 4) if using ONLY a file variable, this is determined from the APP_CONFIG_FILE environment variable (environment variables have higher precedence)
/// 5) Additional logic can be found in shared-deps/src/configuration.rs
use secrecy::Secret;
use serde_aux::field_attributes::deserialize_number_from_string;

use intersect_ingress_proxy_common::configuration::{BrokerSettings, LogLevel};

#[derive(serde::Deserialize, Clone)]
pub struct Settings {
    /// configuration for the broker, which our applications are listening to
    pub broker: BrokerSettings, // TODO make this a Vec<BrokerSettings>
    #[serde(deserialize_with = "deserialize_number_from_string")]
    /// our application's service port number
    pub app_port: u16,
    /// log level of the entire application
    pub log_level: LogLevel,
    /// this should only contain the SYSTEM prefix, i.e. "organization.facility.system"
    pub topic_prefix: String,
    /// username for Basic Authentication
    pub username: String,
    /// password for Basic Authentication
    pub password: Secret<String>,
    /// set to true for developer-unfriendly settings (currently just log formats)
    pub production: bool,
}
