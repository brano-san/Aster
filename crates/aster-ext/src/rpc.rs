use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub id: Value,
    pub result: Result<Value, RpcError>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

pub fn encode_request(request: &Request) -> String {
    let mut object = method(&request.method, request.params.as_ref());
    object["id"] = request.id.clone();
    line(object)
}

pub fn encode_notification(notification: &Notification) -> String {
    line(method(&notification.method, notification.params.as_ref()))
}

pub fn encode_response(response: &Response) -> String {
    line(match &response.result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": response.id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": response.id, "error": error}),
    })
}

fn method(name: &str, params: Option<&Value>) -> Value {
    let mut object = json!({"jsonrpc": "2.0", "method": name});
    if let Some(params) = params {
        object["params"] = params.clone();
    }
    object
}

fn line(object: Value) -> String {
    format!("{object}\n")
}

pub fn decode_message(line: &str) -> Result<Message, String> {
    let line = line.strip_suffix("\r\n").or_else(|| line.strip_suffix('\n')).unwrap_or(line);
    if line.contains(['\n', '\r']) {
        return Err("Expected one JSON-RPC message per line".into());
    }
    let mut object = serde_json::from_str::<serde_json::Map<String, Value>>(line).map_err(|error| error.to_string())?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("Expected JSON-RPC 2.0".into());
    }
    let id = object.remove("id");
    if id.as_ref().is_some_and(|id| !(id.is_null() || id.is_string() || id.is_number())) {
        return Err("Invalid JSON-RPC id".into());
    }
    let result = object.remove("result");
    let error = object.remove("error");
    let params = object.remove("params");
    if let Some(method) = object.remove("method") {
        let Value::String(method) = method else { return Err("Invalid JSON-RPC method".into()) };
        if result.is_some() || error.is_some() || params.as_ref().is_some_and(|p| !(p.is_object() || p.is_array())) {
            return Err("Invalid JSON-RPC call".into());
        }
        return Ok(match id {
            Some(id) => Message::Request(Request { id, method, params }),
            None => Message::Notification(Notification { method, params }),
        });
    }
    let id = id.filter(|_| params.is_none()).ok_or("Invalid JSON-RPC response")?;
    let result = match (result, error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(serde_json::from_value(error).map_err(|error| error.to_string())?),
        _ => return Err("Expected either a JSON-RPC result or error".into()),
    };
    Ok(Message::Response(Response { id, result }))
}

#[cfg(test)]
#[path = "../test/rpc.rs"]
mod tests;
