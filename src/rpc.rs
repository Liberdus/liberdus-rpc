use futures_util::{ StreamExt, SinkExt };
use serde::{Deserialize, Serialize};
use serde_json::Value;
use poem::{handler, web::{ Data, Json }};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashMap;

use crate::{ CrossThreadSharedState, methods, liberdus };

pub type WebScoketManager = Arc<RwLock<HashMap<String, tokio::sync::mpsc::UnboundedSender<serde_json::Value>>>>;

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
    transmitter: Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>,
    subscription_id: Option<String>,
) -> Json<RpcResponse> {
    let method = req.method.clone();
    let id = req.id;

    let result = match method.as_str() {
        "lib_sendTransaction" => methods::lib_send_transaction(req, liberdus).await, 
        "lib_getAccount" => methods::lib_get_account(req, liberdus).await,
        "lib_getTransactionReceipt" => methods::lib_get_transaction_receipt(req, liberdus).await,
        "lib_getMessages" => methods::lib_get_messages(req, liberdus).await,
        "lib_subscribe" => methods::lib_subscribe(req, liberdus, transmitter, subscription_id).await,
        "lib_unsubscribe" => methods::lib_unsubscribe(req, liberdus).await,
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
    rpc_handler(&liberdus, Json(req), None, None).await
}


#[handler]
pub async fn ws_rpc_handler (
    state: Data<&CrossThreadSharedState>,
    ws: poem::web::websocket::WebSocket,
) -> impl poem::IntoResponse {

    let liberdus = Arc::clone(&state.liberdus);
    ws.protocols(vec!["grbaphql-rs", "graphql-transport-ws"])
      .on_upgrade(|mut socket| async move {
        
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
        let (mut sink, mut stream) = socket.split();


        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let _ = sink.send(poem::web::websocket::Message::Text(msg.to_string())).await;
            }
            drop(sink);
            drop(rx);
        });

        let mut subscription_ids = Vec::new();
        while let Some(Ok(poem::web::websocket::Message::Text(msg))) = stream.next().await {
            let subscription_id = generate_uuid();

            let req: RpcRequest = match serde_json::from_str(&msg) {
                Ok(req) => req,
                Err(_) => {
                    let resp = generate_error_response(1, "Invalid JSON".to_string());
                    let json = serde_json::json!({
                        "jsonrpc": resp.jsonrpc,
                        "result": resp.result,
                        "error": resp.error,
                        "id": resp.id,
                    });
                    let _ = tx.send(json);
                    continue;
                }
            };

            if req.method == "lib_subscribe" {
                subscription_ids.push(subscription_id.clone());
            }

            let resp = rpc_handler(&liberdus, Json(req), Some(tx.clone()), Some(subscription_id.clone())).await;
            let json = serde_json::json!({
                "jsonrpc": resp.jsonrpc,
                "result": resp.result,
                "error": resp.error,
                "id": resp.id,
            });
            let _ = tx.send(json);
        }

        for id in subscription_ids {
            liberdus.unsubscribe_chat_room(&id).await;
        }

    })
}

// don't know how fast this is, but this is only used when a new websocket handshake is made
// I don't want to install new crate just to generate UUID
pub fn generate_uuid() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos();

    let random_part_1 = now & 0xFFFF_FFFF_FFFF_FFFF; // Use the lower 64 bits of the timestamp
    let random_part_2 = (now >> 64) & 0xFFFF;        // Use the next 16 bits

    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (random_part_1 & 0xFFFF_FFFF) as u32,            // First 8 hex digits
        (random_part_1 >> 32) as u16,                   // Next 4 hex digits
        0x4000 | ((random_part_2 & 0x0FFF) as u16),     // Version 4 UUID (random)
        0x8000 | ((random_part_2 & 0x3FFF) as u16),     // Variant 1 (RFC 4122)
        (random_part_1 >> 48) as u64                   // Final 12 hex digits
    )
}
