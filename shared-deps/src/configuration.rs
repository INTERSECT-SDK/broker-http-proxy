/// FOR DEVOPS USERS:
/// This file represents common configuration structures used across both applications.
/// Implementation details are in the `get_configuration` function.
use std::{fmt::Display, str::FromStr};

use secrecy::SecretString;
use serde::{de::Error as deError, Deserialize, Deserializer};
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(serde::Deserialize, Copy, Clone, Debug)]
#[repr(u8)]
pub enum Protocol {
    Amqp,
    Mqtt,
}

impl FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "amqp" => Ok(Protocol::Amqp),
            "mqtt" => Ok(Protocol::Mqtt),
            wrong => Err(format!("'{wrong}' is not a valid protocol")),
        }
    }
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Amqp => f.write_str("amqp"),
            Protocol::Mqtt => f.write_str("mqtt"),
        }
    }
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct BrokerSettings {
    /// broker username
    pub username: String,
    /// broker password
    pub password: SecretString,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    /// broker port
    pub port: u16,
    /// broker hostname
    pub host: String,
    pub protocol: Protocol,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

impl FromStr for LogLevel {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "warning" | "warn" => Ok(LogLevel::Warning),
            "error" => Ok(LogLevel::Error),
            // default to info
            _ => Ok(LogLevel::Info),
        }
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => f.write_str("debug"),
            LogLevel::Error => f.write_str("error"),
            LogLevel::Warning => f.write_str("warning"),
            LogLevel::Info => f.write_str("info"),
            LogLevel::Trace => f.write_str("trace"),
        }
    }
}

/// when deserializing, strip out any trailing slashes from the end (useful for URLs)
///
/// # Errors
///   - Deserialization error: if the value can't be deserialized (should generally be unreachable unless value is not valid UTF-8)
pub fn deserialize_trim_trailing_slash<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let mut base: String = Deserialize::deserialize(deserializer)?;
    if base.ends_with('/') {
        base.pop();
    }
    Ok(base)
}

/// custom deserializer which enforces and normalizes system names
///
/// # Errors
///   - Deserialization error: if the value can't be deserialized (only if value is not valid UTF-8) or if the value does not represent a valid INTERSECT namespace
pub fn deserialize_enforce_topic_prefixes<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let mut base: String = Deserialize::deserialize(deserializer)?;

    // TODO will need to update this logic later, when we reduce the namespacing as part of a new INTERSECT release
    let mut period_count = 0_u16;
    for character in base.chars() {
        if character == '-' || character.is_ascii_digit() || character.is_ascii_lowercase() {
            continue;
        }

        if character == '.' {
            period_count += 1;
        } else {
            return Err(deError::custom(format!(
                "topic_prefix: Invalid character detected: {character}",
            )));
        }
    }

    match period_count {
        2 => {
            if base.ends_with('.') {
                Err(deError::custom(
                    "topic_prefix: 3 levels of namespacing expected but only 2 provided",
                ))
            } else {
                base += ".";
                Ok(base)
            }
        }
        3 => {
            if base.ends_with('.') {
                Ok(base)
            } else {
                Err(deError::custom(
                    "topic_prefix: 3 levels of namespacing expected but 4 provided",
                ))
            }
        }
        c => Err(deError::custom(format!(
            "topic_prefix: Incorrect number of namespaces ({c}), there should be 3",
        ))),
    }
}

/// Common logic for obtaining a configuration of type T
///
/// Rules:
/// - highest priority comes from environment variables
/// - lowest priority comes from values in the YAML config file (path determined in `APP_CONFIG_FILE` environment variable)
/// - environment variables can be set for nested structs, i.e. `Settings.broker.port` is set by `PROXYAPP_BROKER__PORT=5001` . Two underscores separate struct values.
/// - while you are not required to provide a single source of configuration and can mix them as you want, note that failure to provide a config value
///   will cause the application to exit with a non-zero code.
///
/// # Errors
///   - Errors if we can't construct the environment from both a config file and environment variables.
pub fn get_configuration<'de, T: serde::Deserialize<'de>>() -> Result<T, config::ConfigError> {
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
        .set_default("production", false)?
        .set_default("log_level", "info")?
        .add_source(file_config)
        // Add in settings from environment variables (with a prefix of PROXYAPP and '__' as separator)
        // E.g. `PROXYAPP_BROKER__PORT=5001 would set `Settings.broker.port`
        .add_source(
            config::Environment::with_prefix("PROXYAPP")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    settings.try_deserialize::<T>()
}
