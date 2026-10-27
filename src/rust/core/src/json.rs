// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

use serde_json::{Map, Value};

/// Whether a JSON Lines line has nothing but whitespace, which readers skip.
pub(crate) fn is_blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

/// The text as a JSON object, skipping a UTF-8 byte order mark. The error says it isn't valid JSON, or isn't an object.
pub(crate) fn object(data: &[u8]) -> Result<Map<String, Value>, String> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    match serde_json::from_slice(data) {
        Ok(Value::Object(obj)) => Ok(obj),
        Ok(_) => Err("not a JSON object".to_owned()),
        Err(e) => Err(format!("not valid JSON: {e}")),
    }
}

/// `text` as a JSON string, escaped as Python's `json.dumps(text, ensure_ascii=False)` escapes it, so files written
/// here match what galaxy-sync writes byte for byte.
pub(crate) fn string(text: &str) -> String {
    serde_json::to_string(text).expect("a str is always valid JSON")
}
