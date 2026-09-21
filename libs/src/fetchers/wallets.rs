use std::pin::Pin;

use serde::Deserialize;
use serde_json::{ json, Value };

use book::{
   debug,
   err_utils::{ ErrStr, err_or },
   list_utils::async_filter_map,
   utils::pred
};
use libs::types::blockchains::Blockchain;

use crate::{
   auto_trading::query_quote,
   clients::http_client,
   consts::DUST_EPSILON,
   hex::{ hex_to_u128, pad_address_for_call },
   types::{
      balances::tokens::{ TokenBalance, mk_token_balance },
      tokens::TokenRegistry
   }
};

//============================================================================
//----- Wallet Balance Check --------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<String>,
    error:  Option<Value>
}

pub async fn fetch_wallet_balances(blockchain: &Blockchain,
                                   registry: &TokenRegistry,
                                   addy: &str, debug: bool)
       -> ErrStr<Vec<TokenBalance>> {
    debug!("fetch_wallet_balances", debug);
    log!("wallet {} on {}", addy, blockchain);

    fn token_fetcher<'a>(b: &'a Blockchain, a: &'a str, r: &'a TokenRegistry,
                         debug: bool)
        -> impl Fn(String)
        -> Pin<Box<dyn Future<Output = ErrStr<Option<(String, f32)>>> + 'a>> {
       move |tok: String| Box::pin(async move {
          let bal = fetch_token_balance(b, a, &tok, r, debug).await?;
          Ok(bal.and_then(|b| Some((tok, b))))
       })
    }
    let tf = token_fetcher(blockchain, addy, registry, debug);
    let toks: Vec<String> =
       registry.as_map().keys().map(String::to_string).collect();
    let opts = async_filter_map(tf, toks).await?;
    let pairs: Vec<(String, f32)> = opts.into_iter().flatten().collect();
    fn build_token_balance<'a>(b: &'a Blockchain, r: &'a TokenRegistry,
                               debug: bool)
          -> impl Fn((String, f32))
          -> Pin<Box<dyn Future<Output=ErrStr<TokenBalance>> + 'a>> {
       move |(tok, bal): (String, f32)| Box::pin(async move {
          let qt = query_quote(b, &r, &tok, debug).await?;
          let bal = mk_token_balance(&tok, qt, bal);
          Ok(bal)
       })
    }
    async_filter_map(build_token_balance(blockchain, registry, debug),
                     pairs).await
}

pub async fn fetch_token_balance(blockchain: &Blockchain, addy: &str, 
                                 symbol: &str, registry: &TokenRegistry,
                                 debug: bool) -> ErrStr<Option<f32>> {
    debug!("fetch_token_balance", debug); 
    let entry = registry.token(symbol)?;
    let raw = if entry.native {
        native_coin_balance(blockchain, addy).await?
    } else {
        let addr = entry.address.ok_or(format!("No address for {symbol}"))?;
        erc20_balance(blockchain, addy, &addr).await?
    };
    let balance = raw as f32 / 10f32.powi(entry.decimals as i32);
    log!("Token {}: {:.8}", symbol, balance);
    Ok(pred(has_balance(balance), balance))
}

fn has_balance(balance: f32) -> bool { balance > DUST_EPSILON }

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

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use libs::types::blockchains::Blockchain::AVALANCHE;
    use crate::fetchers::tokens::fetch_token_registry;

    create_testing!("wallets");

    const TEST_MANDI_ADDRESS: &str =
       "0x6700bD7EAE41434f566e48738813fC585B95669a";

    async fn fetch_balance(hdr: &str, tok: &str) -> ErrStr<()> {
       let ava = &AVALANCHE;
       let registry = fetch_token_registry(ava).await?;
       let balance0 =
          fetch_token_balance(ava, TEST_MANDI_ADDRESS,
                              tok, &registry, true).await?;
       let balance = balance0.unwrap_or(0.0);
       println!("Test wallet {tok}{hdr} balance: {balance:.8}");
       Ok(())
    }

    run!("wallet_balance_btc", now(fetch_balance("", "BTC"))?);
    run!("wallet_balance_undead", now(fetch_balance("", "UNDEAD"))?);
    run!("wallet_balance_avax_native", 
         now(fetch_balance(" (native)", "AVAX"))?);
}
