use super::*;
use serde_json::json;

#[test]
fn request_round_trips_in_a_single_line() {
    let request = Request { id: json!(7), method: "initialize".into(), params: Some(json!({"language": "en\nUS"})) };
    let encoded = encode_request(&request);
    assert!(encoded.ends_with('\n'));
    assert_eq!(encoded.lines().count(), 1);
    assert_eq!(decode_message(&encoded).unwrap(), Message::Request(request));
}

#[test]
fn notification_round_trips_without_an_id() {
    let note = Notification { method: "shutdown".into(), params: None };
    let encoded = encode_notification(&note);
    assert_eq!(decode_message(&encoded).unwrap(), Message::Notification(note));
    assert!(!encoded.contains("\"id\""));
}

#[test]
fn decodes_success_and_error_responses() {
    let response = Response { id: json!("request"), result: Ok(Value::Null) };
    assert_eq!(decode_message(&encode_response(&response)).unwrap(), Message::Response(response));
    let response = Response { id: json!(7), result: Err(RpcError { code: -32601, message: "Method not found".into(), data: None }) };
    assert_eq!(decode_message(&encode_response(&response)).unwrap(), Message::Response(response));
}

#[test]
fn rejects_invalid_frames_and_ambiguous_messages() {
    for line in [
        r#"{"jsonrpc":"1.0","method":"shutdown"}"#,
        r#"{"jsonrpc":"2.0","method":"notify","params":4}"#,
        r#"{"jsonrpc":"2.0","method":"notify","id":true}"#,
        r#"{"jsonrpc":"2.0","method":"notify","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":3}"#,
        r#"{"jsonrpc":"2.0","id":3,"result":{},"error":{"code":1,"message":"bad"}}"#,
        "{\n\"jsonrpc\":\"2.0\",\"method\":\"shutdown\"}",
        "[]",
    ] {
        assert!(decode_message(line).is_err(), "{line}");
    }
}
