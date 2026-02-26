pub mod client;
pub mod receiver;
pub mod sender;

use axum::extract::ws::Message;

pub fn redact_ws_message_for_log(msg: &Message) -> String {
    match msg {
        Message::Text(data) => {
            let mut final_data: String = data.to_string();

            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(track_obj) = json.get_mut("track").and_then(|v| v.as_object_mut()) {
                    track_obj
                        .entry("userData".to_string())
                        .and_modify(|v| *v = serde_json::Value::from("[REDACTED]"));
                    track_obj
                        .entry("pluginInfo".to_string())
                        .and_modify(|v| *v = serde_json::Value::from("[REDACTED]"));

                    if let Some(encoded) = track_obj.get("encoded").and_then(|e| e.as_str()) {
                        if encoded.len() > 100 {
                            let mut c_idx = 100;
                            while !encoded.is_char_boundary(c_idx) {
                                c_idx -= 1;
                            }
                            track_obj.insert(
                                "encoded".to_string(),
                                serde_json::Value::from(format!("{}...", &encoded[..c_idx])),
                            );
                        }
                    }
                }

                if let Ok(redacted) = serde_json::to_string(&json) {
                    final_data = redacted;
                }
            }

            let byte_len = final_data.len();
            if byte_len > 1000 {
                let mut c_idx = 500;
                while !final_data.is_char_boundary(c_idx) {
                    c_idx -= 1;
                }
                format!(
                    "Text({}... (truncated, byte len: {}))",
                    &final_data[..c_idx],
                    byte_len
                )
            } else {
                format!("Text({})", final_data)
            }
        }
        Message::Binary(b) => format!("Binary({} bytes)", b.len()),
        other => format!("{:?}", other),
    }
}
