//! RPC Methods for Interacting with the Liberdus Backend.
//! 
//! This module provides a set of asynchronous functions to handle RPC requests. These functions
//! interact with the `liberdus` library to perform operations such as sending transactions,
//! fetching transaction receipts, managing subscriptions, and more.
//! 
//! # Functions Overview
//! - [`lib_send_transaction`]: Injects a transaction into the Liberdus system with retry logic.
//! - [`lib_get_transaction_receipt`]: Retrieves the receipt of a specific transaction.
//! - [`lib_get_transaction_history`]: Fetches the transaction history for a given account.
//! - [`lib_get_account`]: Fetches account details based on an address.
//! - [`lib_get_messages`]: Retrieves chat messages for a specific chat ID.
//! - [`lib_subscribe`]: Subscribes to a chat room for updates.
//! - [`lib_unsubscribe`]: Unsubscribes from a chat room.

use crate::{liberdus, rpc::{self, RpcRequest}};
use serde_json::Value;
use std::sync::Arc;
use rand::prelude::*;

/// Sends a transaction to the Liberdus backend with retry logic.
///
/// # Parameters
/// - `req`: The RPC request containing the transaction details.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] indicating success or failure.
pub async fn lib_send_transaction(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse  {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx = values[0].clone();
            let max_retry = {
                let mut rng = rand::thread_rng();
                rng.gen_range(3..5)
            };
            let mut counter = 0;
            loop {
                match liberdus.inject_transaction(tx.clone()).await {
                    Ok(result) => {
                        let parsed_result: liberdus::TxInjectRespInner = serde_json::from_value(result.clone()).unwrap();
                        if parsed_result.success {
                            return rpc::generate_success_response(req.id, result);
                        } else {
                            if counter < max_retry {
                                counter += 1;
                                continue;
                            } else {
                                return rpc::generate_error_response(req.id, parsed_result.reason.into(), -32600);
                            }
                        }
                    },
                    Err(e) => {
                        if counter < max_retry {
                            counter += 1;
                            continue;
                        } else {
                            return rpc::generate_error_response(req.id, e.to_string().into(), -32600);
                        }
                    },
                }
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Retrieves the receipt of a specific transaction.
///
/// # Parameters
/// - `req`: The RPC request containing the transaction hash.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] with the transaction receipt or an error.
pub async fn lib_get_transaction_receipt(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() > 0 => {
            let tx_hash = values[0].as_str().unwrap().to_string();
            match liberdus.get_transaction_receipt(&tx_hash).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Fetches the transaction history for a specific account.
///
/// # Parameters
/// - `req`: The RPC request containing the account ID.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] with the transaction history or an error.
pub async fn lib_get_transaction_history(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let account_id = values[0].as_str().unwrap_or("").to_string();
            match liberdus.get_transaction_history(&account_id).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Retrieves account details for a specific address.
///
/// # Parameters
/// - `req`: The RPC request containing the account address.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] with the account details or an error.
pub async fn lib_get_account(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() == 1 => {
            let addr = values[0].as_str().unwrap().to_string();
            match liberdus.get_account_by_addr(&addr).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        Value::Array(values) if values.len() == 2 => {
            let alias = values[1].as_str().unwrap().to_string();
            let alias_address = match liberdus.get_addr_linked_alias(alias).await {
                Ok(address) => address,
                Err(e) => return rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            };

            match liberdus.get_account_by_addr(&alias_address).await {
                Ok(account) => rpc::generate_success_response(req.id, account),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Retrieves chat messages for a specific chat ID.
///
/// # Parameters
/// - `req`: The RPC request containing the chat ID.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] with the messages or an error.
pub async fn lib_get_messages(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let chat_id = values[0].as_str().unwrap_or("").to_string();
            match liberdus.get_messages(&chat_id).await {
                Ok(result) => rpc::generate_success_response(req.id, result),
                Err(e) => rpc::generate_error_response(req.id, e.to_string().into(), -32600),
            }
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Subscribes to a chat room for updates.
///
/// # Parameters
/// - `req`: The RPC request containing the chat ID.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
/// - `transmitter`: An optional channel transmitter for sending subscription updates.
/// - `subscription_id`: An optional subscription ID.
///
/// # Returns
/// An [`rpc::RpcResponse`] indicating success or failure.
pub async fn lib_subscribe(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>, transmitter: Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>, subscription_id: Option<String>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    if transmitter.is_none() {
        return rpc::generate_error_response(req.id, "Use Websocket".into(), -32600);
    }

    if subscription_id.is_none() {
        return rpc::generate_error_response(req.id, "Invalid subscription id".into(), -32600);
    }

    let sub_id = match subscription_id {
        Some(sub_id) => sub_id,
        _ => return rpc::generate_error_response(req.id, "Invalid subscription id".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let chat_id = values[0].as_str().unwrap_or("").to_string();
            
            liberdus.subscribe_chat_room(&chat_id, &sub_id.clone(), transmitter.unwrap()).await;

            rpc::generate_success_response(req.id, sub_id.clone().into())

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}

/// Unsubscribes from a chat room.
///
/// # Parameters
/// - `req`: The RPC request containing the subscription ID.
/// - `liberdus`: A reference to the `liberdus::Liberdus` instance.
///
/// # Returns
/// An [`rpc::RpcResponse`] indicating success or failure.
pub async fn lib_unsubscribe(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let params = match req.params {
        Some(params) => params,
        None => return rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    };

    match params {
        Value::Array(values) if values.len() > 0 => {
            let sub_id = values[0].as_str().unwrap_or("").to_string();
            
            liberdus.unsubscribe_chat_room(&sub_id).await;

            rpc::generate_success_response(req.id, serde_json::Value::Bool(true))

        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into(), -32600),
    }
}


pub async fn lib_get_nodelist(req: rpc::RpcRequest, liberdus: &Arc<liberdus::Liberdus>) -> rpc::RpcResponse {
    let nodelist = liberdus.active_nodelist.read().await;

    rpc::generate_success_response(req.id, serde_json::json!(nodelist.clone()))
}

