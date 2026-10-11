use serde::Deserialize;
use serde_json::{ Value, from_str, json };

use book::{ debug, err_utils::ErrStr };

use libs::types::blockchains::Blockchain;

use crate::{
   clients::http_client,
   types::{ rest::kyber::KyberSwap,  tokens::TokenRegistry }
};

pub async fn query_quote(blockchain: &Blockchain, registry: &TokenRegistry,
                         tok: &str, debug: bool) -> ErrStr<USD> {
   debug!("query_quote", debug);
   let amt = if tok == "USDC" {
      log!("defaulting USDC price to $1.00");
      1.0
   } else {
      query_swap(blockchain, registry, tok, "USDC",
                 1.0, debug).await?.amount_out
   };
   Ok(mk_usd(amt))
}

// 1. Let Serde handle the nested JSON parsing and validation automatically
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KyberResponse {
    data: KyberData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KyberData {
    router_address: String,
    route_summary: Value, // Keeps the inner JSON block intact
}

pub async fn query_swap(
       blockchain: &Blockchain, registry: &TokenRegistry,
       from: &str, to: &str, amount: f32, debug: bool) -> ErrStr<KyberSwap> {
    debug!("query_swap", debug);

    let from_entry = registry.token(from)?;
    let to_entry = registry.token(to)?;

    let token_in = from_entry.address.as_ref()
                             .ok_or_else(|| format!("No address for {from}"))?;
    let token_out = to_entry.address.as_ref()
                            .ok_or_else(|| format!("No address for {to}"))?;

    let amount_in_base =
       (amount * 10f32.powi(from_entry.decimals as i32)).round() as u128;

    // 2. Use reqwest's query params instead of manual string formatting
    let url = format!("{}/routes", api_url(blockchain));
    let query = [("tokenIn", token_in), ("tokenOut", token_out),
                 ("amountIn", &amount_in_base.to_string())];

    let swap = format!("swap {amount} {from} -> {to}");
    log!("I am calling kyber with {}", swap);

    let moz = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";
    let apple = "AppleWebKit/537.36 (KHTML, like Gecko)";
    let chrome = "Chrome/126.0.0.0";
    let safari = "Safari/537.36";
    let agent = format!("{moz} {apple} {chrome} {safari}");

    // 3. Chain reqwest methods directly and parse the JSON structurally
    let res: KyberResponse = http_client()?
        .get(&url)
        .query(&query)
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", &agent)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("KyberSwap route request failed: {e}"))?
        .json() // Automatically handles text/parsing and returns KyberResponse
        .await
        .map_err(|e| format!("Failed to parse Kyber response: {e}"))?;

    let dat = res.data;

    // 4. Extract amount_out from the strongly-typed route_summary field
    let amount_out_str = dat.route_summary.get("amountOut")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Response missing amountOut".to_string())?;

    let raw: u128 =
       err_or(amount_out_str.parse(), "Could not parse amountOut")?;
    let amount_out = raw as f32 / 10f32.powi(to_entry.decimals as i32);

    log!("For {}, ratio is {}", swap, amount_out);

    Ok(KyberSwap {
        amount_out,
        route_summary_raw: dat.route_summary,
        router_address: dat.router_address
    })
}

fn api_url(blockchain: &Blockchain) -> String {
    let base_url = "https://aggregator-api.kyberswap.com";
    format!("{base_url}/{}/api/v1", blockchain.blockchain())
}
