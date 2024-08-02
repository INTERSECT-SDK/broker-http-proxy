use intersect_ingress_proxy_common::configuration::{BrokerSettings, LogLevel};
/// FOR DEVOPS USERS:
/// 1) The root struct is "Settings", follow logic from there
/// 2) integers can be provided as a string in config files or environment variables
/// 3) if using environment variables, see comment in "get_configuration()" as an example of how nesting works
/// 4) if using ONLY a file variable, this is determined from the APP_CONFIG_FILE environment variable (environment variables have higher precedence)
/// 5) Additional logic can be found in shared-deps/src/configuration.rs
use secrecy::Secret;

#[derive(serde::Deserialize, Clone)]
pub struct ExternalProxy {
    /// URL for the other ingress proxy we are communicating with
    pub url: String,
    /// Basic authentication credentials for the other proxy
    pub username: String,
    /// Basic authentication credentials for the other proxy
    pub password: Secret<String>,
}

#[derive(serde::Deserialize, Clone)]
pub struct Settings {
    /// configuration for the broker, which our applications are listening to
    pub broker: BrokerSettings, // TODO make this a Vec<BrokerSettings>
    /// URL for the other ingress proxy we are communicating with
    pub other_proxy: ExternalProxy, // TODO make this a Vec<ExternalProxy>
    /// log level for the entire application
    pub log_level: LogLevel,
    /// set to true for developer-unfriendly settings (currently just log formats)
    pub production: bool,
}
