use serde::Deserialize;
use serde_json::{ json, Value };

use book::err_utils::{ ErrStr, err_or };
use libs::types::blockchains::Blockchain;

use super::{
   clients::http_client,
   hex::{ hex_to_u128, pad_address_for_call },
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
    let resp =
       err_or(http_client()?.post(&blockchain.url()).json(&body).send().await,
              &format!("RPC request ({method}) failed"))?;
    let parsed: RpcResponse =
       err_or(resp.json().await,
              &format!("RPC response for {method} did not parse"))?;
    if let Some(err) = parsed.error {
       Err(format!("RPC error for {method}: {err}"))
    } else {
       parsed.result.ok_or(format!("RPC call {method} returned no result"))
    }
}

async fn erc20_balance(blockchain: &Blockchain, addy: &str, contract: &str)
      -> ErrStr<u128> {
    // balanceOf(address) selector = 0x70a08231
    let data = format!("0x70a08231{}", pad_address_for_call(addy));
    let query = json!([{ "to" : contract, "data": data }, "latest"]);
    let result = rpc_call(blockchain, "eth_call", query).await?;
    hex_to_u128(&result)
}

async fn native_coin_balance(blockchain: &Blockchain, addy: &str)
      -> ErrStr<u128> {
    let result =
       rpc_call(blockchain, "eth_getBalance", json!([addy, "latest"])).await?;
    hex_to_u128(&result)
}

pub async fn wallet_balance(blockchain: &Blockchain, addy: &str, symbol: &str,
                            registry: &TokenRegistry) -> ErrStr<f64> {
    let entry = registry.token(symbol)?;
    let raw = if entry.native {
        native_coin_balance(blockchain, addy).await?
    } else {
        let addr = entry.address.ok_or(format!("No address for {symbol}"))?;
        erc20_balance(blockchain, addy, &addr).await?
    };
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
        let balance =
           now(wallet_balance(&AVALANCHE, "0x123", "UNDEAD", &registry))?;
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
    });
}
