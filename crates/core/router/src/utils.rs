use tracing::{event, Level};
use serde_json::json;

/// Logs a structured JSON event with the given level, message, and key-value pairs.
/// Example usage:
/// json_log(Level::INFO, "Swap executed", &[("swap_id", &swap_id), ("profit", &profit)]);
pub fn json_log(level: Level, message: &str, fields: &[(&str, &str)]) {
    let mut map = serde_json::Map::new();
    for (key, value) in fields {
        map.insert(key.to_string(), json!(value));
    }
    map.insert("message".to_string(), json!(message));
    let json_value = serde_json::Value::Object(map);

    match level {
        Level::ERROR => event!(Level::ERROR, %json_value),
        Level::WARN => event!(Level::WARN, %json_value),
        Level::INFO => event!(Level::INFO, %json_value),
        Level::DEBUG => event!(Level::DEBUG, %json_value),
        Level::TRACE => event!(Level::TRACE, %json_value),
    }
}
