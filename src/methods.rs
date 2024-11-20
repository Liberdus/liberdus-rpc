use crate::rpc;
use serde_json::Value;

pub async fn handle_add(req: rpc::RpcRequest) -> rpc::RpcResponse {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() == 2 => {
            let result = values[0].as_i64().unwrap_or(0) + values[1].as_i64().unwrap_or(0);
            rpc::generate_success_response(req.id, Value::from(result))
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into()),
    }
}

pub async fn handle_subtract(req: rpc::RpcRequest) -> rpc::RpcResponse {
    let params = req.params.unwrap_or(Value::Null);
    match params {
        Value::Array(values) if values.len() == 2 => {
            let result = values[0].as_i64().unwrap_or(0) - values[1].as_i64().unwrap_or(0);
            rpc::generate_success_response(req.id, Value::from(result))
        }
        _ => rpc::generate_error_response(req.id, "Invalid parameters".into()),
    }
}

