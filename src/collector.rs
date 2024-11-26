use serde_json;

pub fn get_transaction(id: &String) -> Result<serde_json::Value, serde_json::Value> {
    // in actuality rpc should be opaque to whatever the collector give and just simply pass it
    // back towards client
    // check liberdus::get_transaction for examples.

    todo!();

    return Ok(serde_json::json!({
        "foo": "bar",
    }))
}
