use serde::Deserialize;
use serde_json::Value;

use book::{
   debug,
   currency::usd::{ USD, mk_usd },
   err_utils::{ ErrStr, err_or }
};

use libs::types::blockchains::Blockchain;

use crate::{
   implementation::clients::http_client,
   types::{ rest::kyber::KyberSwap,  tokens::TokenRegistry }
};
use super::utils::{ u128_2_f32, f32_2_u128 };

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

    let amount_in_base = f32_2_u128(amount, from_entry.decimals as i32);

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
       err_or(amount_out_str.parse(),
              &format!("Could not parse amountOut: '{amount_out_str}'"))?;
    let amount_out = u128_2_f32(raw, to_entry.decimals as i32);

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

//============================================================================
//----- TESTS -------------------------------------------------------------
//============================================================================

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, string_utils::UppercaseString, utils::now };
   use libs::types::blockchains::Blockchain::AVALANCHE;
   use crate::implementation::fetchers::tokens::fetch_token_registry;

   create_testing!("implementation::transactions::quotes");

   const AVA: &Blockchain = &AVALANCHE;

   async fn quote_for(token: UppercaseString) -> ErrStr<()> {
      let reg = fetch_token_registry(AVA).await?;
      let quote = query_quote(AVA, &reg, &token, true).await?;
      println!("{token} quote: {quote}");
      Ok(())
   }
   async fn ratio_of(primary: UppercaseString, pivot: UppercaseString)
         -> ErrStr<()> {
      let reg = fetch_token_registry(AVA).await?;
      let ans = query_swap(AVA, &reg, &primary, &pivot, 1.0, true).await?;
      println!("The {primary}/{pivot} ratio is: {ans:?}");
      Ok(())
   }

   run!("btc_quote", now(quote_for("btc".into()))?);
   run!("undead_quote", now(quote_for("undead".into()))?);
   run!("btc_eth_ratio", now(ratio_of("btc".into(), "eth".into()))?);
   run!("btc_undead_ratio", now(ratio_of("btc".into(), "undead".into()))?);
}
