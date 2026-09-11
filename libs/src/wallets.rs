use std::time::Duration;
use serde::Deserialize;
use serde_json::{ json, Value };

use book::err_utils::ErrStr;

use super::{
   clients::http_client,
   hex::pad_address_for_call,
   types::tokens::TokenRegistry
};

//============================================================================
//----- Wallet Balance Check --------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<String>,
    error:  Option<Value>
}

async fn rpc_call(blockchain: &Blockchain, method: &str, params: Value)
      -> ErrStr<String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let resp = http_client()?
        .post(&blockchain.url())
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("RPC request ({method}) failed: {e}"))?;
    let parsed: RpcResponse = resp
        .json()
        .await
        .map_err(|e| format!("RPC response for {method} did not parse: {e}"))?;
    if let Some(err) = parsed.error {
        return Err(format!("RPC error for {method}: {err}"));
    }
    parsed.result.ok_or(format!("RPC call {method} returned no result"))
}

fn hex_to_u128(hex: &str) -> ErrStr<u128> {
    let trimmed = hex.trim_start_matches("0x");
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    u128::from_str_radix(trimmed, 16)
        .map_err(|e| format!("Could not parse hex balance '{hex}': {e}"))
}

async fn erc20_balance(addy: &str, contract: &str) -> ErrStr<u128> {
    // balanceOf(address) selector = 0x70a08231
    let data = format!("0x70a08231{}", pad_address_for_call(addy));
    let query = json!([{ "to" : contract, "data": data }, "latest"]);
    let result = rpc_call("eth_call", query).await?;
    hex_to_u128(&result)
}

async fn native_coin_balance(addy: &str) -> ErrStr<u128> {
    let result = rpc_call("eth_getBalance", json!([addy, "latest"])).await?;
    hex_to_u128(&result)
}

pub async fn wallet_balance(addy: &str, symbol: &str, registry: &TokenRegistry)
      -> ErrStr<f64> {
    let entry = registry.token(symbol)?;
    let raw = if entry.native {
        native_coin_balance(addy)
    } else {
        let addr = entry
            .address
            .as_deref()
            .ok_or(format!("'{symbol}' is not marked native and has no address in tokens.toml — add one or set native = true"))?;
        erc20_balance(addy, addr)
    }.await?;
    Ok(raw as f64 / 10f64.powi(entry.decimals as i32))
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use libs::types::blockchains::Blockchain::AVALANCHE;
    use crate::fetchers::tokens::fetch_tokens;

    create_testing!("wallets");

    run!("wallet_balance_undead", {
        let registry = now(fetch_tokens(&AVALANCHE))?;
        let balance = now(wallet_balance("0x123", "UNDEAD", &registry))?;
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
    });
}
