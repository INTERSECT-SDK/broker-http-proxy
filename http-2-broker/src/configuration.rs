/// FOR DEVOPS USERS:
/// 1) The root struct is "Settings", follow logic from there
/// 2) integers can be provided as a string in config files or environment variables
/// 3) if using environment variables, see comment in "get_configuration()" as an example of how nesting works
/// 4) if using ONLY a file variable, this is determined from the APP_CONFIG_FILE environment variable (environment variables have higher precedence)
use intersect_ingress_proxy_common::configuration::{BrokerSettings, LogLevel};

#[derive(serde::Deserialize, Clone)]
pub struct Settings {
    /// configuration for the broker, which our applications are listening to
    pub broker: BrokerSettings, // TODO make this a Vec<BrokerSettins>
    /// URL for the other ingress proxy we are communicating with
    pub other_proxy_url: String, // TODO make this a Vec<String>
    /// log level for the entire application
    pub log_level: LogLevel,
    /// set to true for developer-unfriendly settings (currently just log formats)
    pub production: bool,
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let file_config = match std::env::var("APP_CONFIG_FILE") {
        Ok(config_file_path) => config::Config::builder()
            .add_source(config::File::new(
                &config_file_path,
                config::FileFormat::Yaml,
            ))
            .build()?,
        Err(_) => config::Config::builder().build()?,
    };

    let settings = config::Config::builder()
        .add_source(file_config)
        // Add in settings from environment variables (with a prefix of HTTP2BROKER and '__' as separator)
        // E.g. `HTTP2BROKER_BROKER__PORT=5001 would set `Settings.broker.port`
        .add_source(
            config::Environment::with_prefix("HTTP2BROKER")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    settings.try_deserialize::<Settings>()
}
