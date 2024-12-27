//! This module provides utilities to handle RPC calls for interacting with a backend system.
//! 
//! The module includes the following functionalities:
//! - HTTP and WebSocket RPC handlers.
//! - Structured request and response models for JSON-RPC 2.0.
//! - Helper functions to generate success and error responses.
//! - A UUID generator for WebSocket subscription management.
//! 
//! # Structures
//! - [`RpcRequest`]: Represents an incoming JSON-RPC request.
//! - [`RpcResponse`]: Represents a JSON-RPC response.
//! - [`RpcError`]: Represents an error response for JSON-RPC.
//! 
//! # Functions
//! - [`generate_success_response`]: Creates a JSON-RPC success response.
//! - [`generate_error_response`]: Creates a JSON-RPC error response.
//! - [`rpc_handler`]: Handles RPC requests and routes them to appropriate methods.
//! - [`http_rpc_handler`]: HTTP entry point for handling JSON-RPC requests.
//! - [`ws_rpc_handler`]: WebSocket entry point for handling JSON-RPC requests.
//! - [`generate_uuid`]: Generates a UUID-like string for WebSocket subscription management.

use futures_util::{ StreamExt, SinkExt };
use serde::{Deserialize, Serialize};
use serde_json::Value;
use poem::{handler, web::{ Data, Json }};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{ CrossThreadSharedState, methods, liberdus };

/// Represents an incoming JSON-RPC request.
#[derive(Debug, Deserialize, Serialize)]
pub struct RpcRequest {
    /// JSON-RPC protocol version.
    pub jsonrpc: String,

    /// The name of the method being invoked.
    pub method: String,

    /// Parameters for the method call, if any.
    pub params: Option<Value>,

    /// Unique identifier for the request.
    pub id: u32,
}

/// Represents a JSON-RPC response.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    /// JSON-RPC protocol version.
    pub jsonrpc: String,

    /// The result of the method call, if successful.
    pub result: Option<Value>,

    /// The error object, if the method call failed.
    pub error: Option<RpcError>,

    /// Unique identifier for the response.
    pub id: u32,
}

/// Represents an error response for JSON-RPC.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    /// Error code.
    code: i32,

    /// Error message.
    message: String,
}

/// Creates a JSON-RPC success response.
///
/// # Parameters
/// - `id`: The ID of the request.
/// - `result`: The result of the method call.
///
/// # Returns
/// A [`RpcResponse`] object representing success.
pub fn generate_success_response(id: u32, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

/// Creates a JSON-RPC error response.
///
/// # Parameters
/// - `id`: The ID of the request.
/// - `error_msg`: The error message to include in the response.
/// - `code`: The error code.
///
/// # Returns
/// A [`RpcResponse`] object representing an error.
pub fn generate_error_response(id: u32, error_msg: String, code: i32) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(RpcError {
            code,
            message: error_msg,
        }),
        id,
    }
}

/// Handles RPC requests by routing them to the appropriate methods.
///
/// # Parameters
/// - `liberdus`: Shared state containing the backend interface.
/// - `req`: The incoming JSON-RPC request.
/// - `transmitter`: Optional channel for WebSocket communication.
/// - `subscription_id`: Optional subscription ID for WebSocket management.
///
/// # Returns
/// A [`RpcResponse`] containing the result or error of the method call.
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
        "lib_getTransactionHistory" => methods::lib_get_transaction_history(req, liberdus).await,
        "lib_getMessages" => methods::lib_get_messages(req, liberdus).await,
        "lib_subscribe" => methods::lib_subscribe(req, liberdus, transmitter, subscription_id).await,
        "lib_unsubscribe" => methods::lib_unsubscribe(req, liberdus).await,
        _ => generate_error_response(id, "Method not found".to_string(), -32601),
    };

    Json(result)
}

/// HTTP entry point for handling JSON-RPC requests.
///
/// This handler processes incoming HTTP requests containing JSON-RPC payloads and forwards them to the RPC handler.
///
/// # Parameters
/// - `state`: Shared state containing the backend interface.
/// - `req`: The incoming JSON-RPC request.
///
/// # Returns
/// A [`RpcResponse`] containing the result or error of the method call.
#[handler]
pub async fn http_rpc_handler (
    state: Data<&CrossThreadSharedState>,
    Json(req): Json<RpcRequest>,
) -> Json<RpcResponse> {
    let liberdus = Arc::clone(&state.liberdus);
    rpc_handler(&liberdus, Json(req), None, None).await
}

/// WebSocket entry point for handling JSON-RPC requests.
///
/// This handler establishes a WebSocket connection, processes incoming JSON-RPC messages, and sends responses back.
/// It supports subscription management via UUID generation.
///
/// # Parameters
/// - `state`: Shared state containing the backend interface.
/// - `ws`: The WebSocket connection.
///
/// # Returns
/// A WebSocket response handler.
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
                    let resp = generate_error_response(1, "Invalid JSON".to_string(), -32700);
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

/// Generates a UUID-like string based on the current timestamp.
///
/// This function avoids external dependencies by using the system time to create a unique identifier.
///
/// # Returns
/// A string formatted as a UUID.
///
/// # Notes
/// - This approach relies on the system clock and may produce duplicates if called in rapid succession.
/// - The generated UUID follows the general structure of version 4 UUIDs but is not guaranteed to be globally unique.
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

