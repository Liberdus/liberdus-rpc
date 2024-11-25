use serde::{Deserialize, Serialize};
use serde_json::Value;
use poem::{handler, web::{ Data, Json }};
use std::sync::Arc;

use crate::{ CrossThreadSharedState, methods };


#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: u32,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub id: u32,
}

pub fn generate_success_response(id: u32, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

pub fn generate_error_response(id: u32, error_msg: String) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(error_msg),
        id,
    }
}

#[handler]
pub async fn rpc_handler(
    state: Data<&CrossThreadSharedState>,
    Json(req): Json<RpcRequest>,
) -> Json<RpcResponse> {
    let method = req.method.clone();
    let id = req.id;

    let result = match method.as_str() {
        "lib_sendTransaction" => methods::lib_send_transaction(req, Arc::clone(&state.liberdus)).await, 
        "lib_getAccount" => methods::lib_get_account(req, Arc::clone(&state.liberdus)).await,
        "lib_getTransactionReceipt" => methods::lib_get_transaction_receipt(req, Arc::clone(&state.liberdus)).await,
        "lib_getMessage" => methods::lib_get_messages(req, Arc::clone(&state.liberdus)).await,
        _ => generate_error_response(id, "Method not found".to_string()),
    };

    Json(result)

}

