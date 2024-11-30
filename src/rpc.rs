use futures_util::{ StreamExt, SinkExt };
use serde::{Deserialize, Serialize};
use serde_json::Value;
use poem::{handler, web::{ Data, Json }};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use crate::{ CrossThreadSharedState, methods, liberdus };

type WebScoketManager = Arc<RwLock<HashMap<String, poem::web::websocket::WebSocketStream>>>;

#[derive(Debug, Deserialize, Serialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: u32,
}

#[derive(Debug, Serialize, Deserialize)]
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

async fn rpc_handler(
    liberdus: &Arc<liberdus::Liberdus>,
    Json(req): Json<RpcRequest>,
    _ws: Option<poem::web::websocket::WebSocketStream>,
) -> Json<RpcResponse> {
    let method = req.method.clone();
    let id = req.id;

    let result = match method.as_str() {
        "lib_sendTransaction" => methods::lib_send_transaction(req, liberdus).await, 
        "lib_getAccount" => methods::lib_get_account(req, liberdus).await,
        "lib_getTransactionReceipt" => methods::lib_get_transaction_receipt(req, liberdus).await,
        "lib_getMessage" => methods::lib_get_messages(req, liberdus).await,
        _ => generate_error_response(id, "Method not found".to_string()),
    };

    Json(result)

}


#[handler]
pub async fn http_rpc_handler (
    state: Data<&CrossThreadSharedState>,
    Json(req): Json<RpcRequest>,
) -> Json<RpcResponse> {
    let liberdus = Arc::clone(&state.liberdus);
    rpc_handler(&liberdus, Json(req), None).await
}


#[handler]
pub async fn ws_rpc_handler (
    state: Data<&CrossThreadSharedState>,
    ws: poem::web::websocket::WebSocket,
) -> impl poem::IntoResponse {

    let liberdus = Arc::clone(&state.liberdus);
    ws.protocols(vec!["grbaphql-rs", "graphql-transport-ws"])
      .on_upgrade(|mut socket| async move {

        while let Some(Ok(poem::web::websocket::Message::Text(msg))) = socket.next().await {

            let req: RpcRequest = match serde_json::from_str(&msg) {
                Ok(req) => req,
                Err(_) => {
                    let resp = generate_error_response(0, "Invalid JSON".to_string());
                    let _ = socket.send(poem::web::websocket::Message::Text(serde_json::to_string(&resp).unwrap())).await;
                    continue;
                }
            };

            let resp = rpc_handler(&liberdus, Json(req), None).await;
            let json = serde_json::json!({
                "jsonrpc": resp.jsonrpc,
                "result": resp.result,
                "error": resp.error,
                "id": resp.id,
            });
            let _ = socket.send(poem::web::websocket::Message::Text(json.to_string())).await;
        }

    })
}
