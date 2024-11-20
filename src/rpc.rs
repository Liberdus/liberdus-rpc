use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_http::{Request, Response};
use crate::methods;

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub id: Option<Value>,
}

pub fn generate_success_response(id: Option<Value>, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

pub fn generate_error_response(id: Option<Value>, error_msg: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(error_msg),
        id,
    }
}


pub async fn handle_request(mut request: Request) {
    let body = {
        let mut buffer = Vec::new();
        request.as_reader().read_to_end(&mut buffer).unwrap_or(0);
        String::from_utf8(buffer).unwrap_or_default()
    };

    let rpc_request: Result<RpcRequest, _> = serde_json::from_str(&body);
    let rpc_response = match rpc_request {
        Ok(req) => match req.method.as_str() {
            "add" => methods::handle_add(req).await,
            "subtract" => methods::handle_subtract(req).await,
            _ => generate_error_response(req.id, "Method not found".into()),
        },
        Err(_) => generate_error_response(None, "Invalid JSON".into()),
    };

    let response_body = serde_json::to_string(&rpc_response).unwrap_or_else(|_| {
        serde_json::json!({
            "jsonrpc": "2.0",
            "error": "Internal Server Error",
            "id": null
        })
        .to_string()
    });

    let response = Response::from_string(response_body).with_header(
        tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
    );
    request.respond(response).unwrap_or_else(|e| {
        eprintln!("Failed to send response: {:?}", e);
    });
}
