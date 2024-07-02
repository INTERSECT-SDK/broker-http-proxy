/// This module contains all of the core INTERSECT logic regarding messages.
use serde::Deserialize;

/// should use a non-printable delimiter which should not appear in a channel definition, but is also not the EOF character.
/// Note that chars in Rust assume valid UTF-8. UTF-8 is probably the most efficient approach.
/// If we're able to switch to encoding the entire message in raw bytes (UTF-8), we should do so - this allows us to handle more data types.
/// Currently, all SSE APIs seem to mandate using Strings, though...
const DELIMITER: char = '\x01';

/// idea is that INTERSECT will just use one AMQP exchange for everything, things get separated based off of the routing key
pub const INTERSECT_MESSAGE_EXCHANGE: &str = "intersect-messages";

#[derive(Deserialize)]
pub struct IntersectMessage {
    // headers: HashMap<String, String>,
    /// All IntersectMessages have headers, and they are the only property relevant to us.
    headers: IntersectMessageHeaders,
}

#[derive(Deserialize)]
pub struct IntersectMessageHeaders {
    /// All IntersectMessages have a data source, which is the only property relevant to us.
    source: String,
}

/// we only use this for broker-2-http - only emit messages from our system through SSE
/// If Result.Error - JSON serialization failure, so do not send it through
/// If Result.OK - JSON serialization success, wrapped boolean determines whether or not to send it through
pub fn should_message_passthrough(
    msg_str: &str,
    this_system: &str,
) -> Result<bool, serde_json::Error> {
    let msg: IntersectMessage = serde_json::from_str(msg_str)?;
    Ok(msg.headers.source.starts_with(this_system))
}

// A NOTE REGARDING THE HTTP EVENTSOURCE STRINGS:
// The values are just the channel concatenated with the message string, separated by a non-printable byte (1)
// since channels always follow a specific format, but messages can have many arbitrary characters in them, list the channel first.

/// build the event source data string
pub fn make_eventsource_data(channel: &str, msg_str: &str) -> String {
    format!("{}{}{}", channel, DELIMITER, msg_str)
}

/// returns a tuple of the channel string and the event source string
pub fn extract_eventsource_data(data: &str) -> (String, String) {
    match data.split_once(DELIMITER) {
        Some((channel, msg_str)) => (channel.to_owned(), msg_str.to_owned()),
        None => {
            tracing::warn!("Data from SSE does not match expected format: {}", data);
            (String::new(), String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_true_if_this_system_in_message() {
        let sample_message = r#"{"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"hello-organization.hello-facility.hello-system.hello-subsystem.hello-service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}"#;

        let this_system = "hello-organization.hello-facility.hello-system";

        let result = should_message_passthrough(sample_message, this_system);
        assert!(result.is_ok());
        assert!(result.unwrap() == true);
    }

    #[test]
    fn should_return_false_if_this_system_not_in_message() {
        let sample_message = r#"{"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"hello-organization.hello-facility.hello-system.hello-subsystem.hello-service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}"#;

        let this_system = "bye-organization.bye-facility.bye-system";

        let result = should_message_passthrough(sample_message, this_system);
        assert!(result.is_ok());
        assert!(result.unwrap() == false);
    }

    #[test]
    fn should_error_if_cant_serialize() {
        let sample_message = r#"{{[}[}}"#;

        let this_system = "hello-organization.hello-facility.hello-system";

        let result = should_message_passthrough(sample_message, this_system);
        assert!(result.is_err());
    }

    #[test]
    fn encode_decode_eventsource_msg_idempotent() {
        let channel = "channel";
        let message = "mess\x01age\x01"; // message also uses delimiter but can still be restored exactly as-is

        let encoded = make_eventsource_data(channel, message);
        // our delimiter only adds one byte to all the data we want to push through
        assert!(encoded.len() == channel.len() + message.len() + 1);

        let (decoded_channel, decoded_message) = extract_eventsource_data(&encoded);
        assert_eq!(channel, decoded_channel);
        assert_eq!(message, decoded_message);
    }
}
