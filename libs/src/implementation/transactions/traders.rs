use std::str::FromStr;

use serde::Deserialize;

use ethers::{
   middleware::SignerMiddleware,
   providers::{ Http, Provider },
   signers::LocalWallet,
   types::{ Address, Bytes, U256 }
};
use serde_json::{ Value, from_str, json };

use book::{
   debug,
   err_utils::{ ErrStr, err_or },
   string_utils::s,
   utils::{ get_env, pred }
};
use libs::types::blockchains::Blockchain;

use crate::{
   implementation::{
      clients::http_client,
      hex::{ pad_address_for_call, pad_u256_for_call },
      fetchers::wallets::fetch_token_balance,
      transactions::{
         utils::{
            api_url,
            build_tx_with_fee_buffer,
            complete_transaction,
            load_signer
         }
      }
   },
   types::{
      quotes::fetch_active_quotes,
      tokens::TokenRegistry,
      trades::{ Trade, Trade::* }
   }
};

//============================================================================
//----- Shared Trade-Attempt Helper --------------------------------------------
//============================================================================

fn slippage_adjusted_floor(min_floor: f32, slippage_bps: u16) -> f32 {
    min_floor / (1.0 - slippage_bps as f32 / 10_000.0)
}

async fn query_swap(blockchain: &Blockchain, registry: &TokenRegistry,
                    from: &str, to: &str, amount: f32, debug: bool)
      -> ErrStr<f32> {
   let auth = get_env("COIN_GECKO_API_KEY")?;
   let quotes = fetch_active_quotes(registry, blockchain, &auth, debug).await?;
   let fq = quotes.lookup(from)?;
   let tq = quotes.lookup(to)?;
   Ok(fq * amount / tq)
}

/// Quotes, checks the floor, and (unless dry-running) executes — the one
/// trade-attempt pipeline every pivot open/close in this system goes
/// through, regardless of which binary is calling it.
pub async fn check_floor(blockchain: &Blockchain, registry: &TokenRegistry,
                         from: &str, to: &str, amount: f32, min_floor: f32,
                         slippage_bps: u16, debug: bool) -> ErrStr<f32> {
    let guaranteed_floor = slippage_adjusted_floor(min_floor, slippage_bps);
    let swap = query_swap(blockchain, registry, from, to, amount, debug).await?;
    pred(swap > guaranteed_floor, swap)
        .ok_or(debug_trade_result(None, "NOT CLEARED", from, to, amount,
                                  swap, min_floor, debug))
}

pub async fn trade(blockchain: &Blockchain, addy: &str,
                   registry: &TokenRegistry,
                   from: &str, amount: f32, to: &str,
                   min_floor: f32, slippage_bps: u16, swap: f32,
                   keystore_path: &str, debug: bool) -> ErrStr<Trade> {
   debug!("trade", debug);
   fn n(x: Option<f32>) -> f32 { x.unwrap_or(0.0) }
   let balance_before =
      fetch_token_balance(blockchain, addy, to, registry, debug).await?;
   let (tx_hash, gas_avax) =
      execute_trade(blockchain, addy, registry, from, to, amount, min_floor,
                    slippage_bps, keystore_path, debug).await?;
   let balance_after =
      fetch_token_balance(blockchain, addy, to, registry, debug).await?;
   let actual_received = n(balance_after) - n(balance_before);
   debug_trade_result(Some(&tx_hash), "EXECUTED", from, to, amount, swap, 
                      min_floor, debug);
   let trade = EXECUTED { tx_hash, actual_received, gas_avax };
   log!("Trade result: {}", format!("{:?}", trade));
   Ok(trade)
}

fn debug_trade_result(tx: Option<&str>, kind: &str, from: &str, to: &str,
                      amount: f32, swap: f32, min_floor: f32,
                      debug: bool) -> String {
   debug!("attempt_trade_with_actual_amount", debug);
   let trade = format!("Trade {from} -> {to}");
   let mb_tx = tx.and_then(|t| Some(format!("tx {t}"))).unwrap_or_default();
   let swap = format!("swap {amount:.4} -> {:.4}", swap);
   let floor = format!("(floor {min_floor:.4})");
   let line = format!("{trade} {mb_tx} {swap} {floor}");
   let ans = format!("[{kind}] {line}");
   log!("{}", ans);
   ans
}

/// allowance. The router can never pull more than what's approved here.
pub async fn approve_exact_amount(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>,
        token_contract: &str,
        spender: &str,
        amount_base_units: u128,
        debug: bool) -> ErrStr<f32> {
    debug!("approve_exact_amount", debug);

    let data_hex = format!(
        "0x095ea7b3{}{}",
        pad_address_for_call(spender),
        pad_u256_for_call(amount_base_units)
    );
    let to = err_or(Address::from_str(token_contract), "Bad token address")?;
    let data = err_or(Bytes::from_str(&data_hex), "Bad approve calldata")?;
    let tx = build_tx_with_fee_buffer(client, to, data, None).await?;
    let (_tx, gas) =
       complete_transaction(client, tx, "approve", false, debug).await?;
    Ok(gas)
}

/// Asks KyberSwap to encode the actual swap calldata for the route.
/// When `debug`, prints the raw response so it can be eyeballed before
/// trusting it — otherwise that's a screen-filling blob of hex calldata,
/// so it stays silent by default. `slippage_bps` is basis points (e.g.
/// 50 = 0.50%).
pub async fn kyberswap_build(blockchain: &Blockchain, route_summary_raw: &Value,
                             sender: &str, slippage_bps: u16, debug: bool)
      -> ErrStr<(String, String)> {
    debug!("kyberswap_build", debug);
    let body = json!({
        "routeSummary": route_summary_raw,
        "sender": sender,
        "recipient": sender,
        "slippageTolerance": slippage_bps
    });

    let resp = err_or(http_client()?
        .post(format!("{}/route/build", api_url(blockchain)))
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await,
        "KyberSwap build request failed")?;

    let status = resp.status();
    let raw_body = err_or(resp.text().await, "Could not read build response")?;
    log!("KyberSwap build response (verify this looks right)");
    log!("{}", raw_body);
    let parsed: Value = err_or(from_str(&raw_body),
        &format!("KyberSwap build response did not parse (HTTP {status})
Raw body: {raw_body}"))?;
    let data = parsed
        .get("data")
        .ok_or(format!("Build response has no data. Raw: {raw_body}"))?;
    let router = data
        .get("routerAddress")
        .and_then(|v| v.as_str())
        .ok_or(format!("Build response missing routerAddress. Raw: {raw_body}"))?
        .to_string();
    let calldata = data
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or(format!("Build response missing calldata. Raw: {raw_body}"))?
        .to_string();

    Ok((router, calldata))
}

/// Signs and sends the swap transaction. Returns the tx hash on success;
/// hard errors on revert, drop, or replacement rather than reporting a
/// false success. To see it: snowtrace.io/tx/<tx_hash>
pub async fn send_swap_tx(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>,
        router: &str, calldata_hex: &str, mb_val: Option<U256>,
        debug: bool) -> ErrStr<(String, f32)> {
    let to = err_or(Address::from_str(router), "Bad router address")?;
    let data =
         err_or(Bytes::from_str(calldata_hex), "Bad calldata from KyberSwap")?;
    let tx = build_tx_with_fee_buffer(client, to, data, mb_val).await?;
    complete_transaction(client, tx, "swap", true, debug).await
}

pub async fn execute_trade(blockchain: &Blockchain, addy: &str,
                           registry: &TokenRegistry, from: &str, to: &str,
                           amount: f32, min_floor: f32, slippage_bps: u16,
                           keystore_path: &str, debug: bool)
      -> ErrStr<(String, f32)> {
    debug!("execute_trade", debug);
    log!("Trading in progress — approve/quote/swap. This part takes a minute.");
    log!(">>> Re-checking the quote after keystore unlock (it may have moved)");
    let ratio =
       query_swap(blockchain, registry, from, to, amount, debug).await?;
    let trade = format!("{amount:.6} {from} -> {:.8} {to}", ratio);
    log!("Fresh quote: {}", trade);
    // See slippage_adjusted_floor: the swap below is authorized (via
    // slippage_bps) to settle as low as fresh_quote * (1 - slippage_bps),
    // so the fresh quote itself must clear that worse case, not just
    // min_floor, or a real close can settle under floor.
    
    let guaranteed_floor = slippage_adjusted_floor(min_floor, slippage_bps);
    if ratio < guaranteed_floor {
       Err(format!("Quote moved below your floor while unlocking the keystore
({:.8} {to} quoted, but only {:.8} {to} is guaranteed at {slippage_bps} bps
slippage tolerance -- need > {min_floor:.8} {to}).
        
That's not happening. No funds used.", ratio,
           ratio * (1.0 - slippage_bps as f32 / 10_000.0)
        )) 
    } else { 
       execute_trade_continuation(blockchain, addy, keystore_path,
                                  registry, from, to, amount,
                                  slippage_bps, debug).await
    }
}

use crate::types::rest::kyber::KyberSwap;
use super::utils::{ f32_2_u128, u128_2_f32 };

// 1. Let Serde handle the nested JSON parsing and validation automatically
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KyberResponse {
    data: KyberData
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KyberData {
    router_address: String,
    route_summary: Value // Keeps the inner JSON block intact
}

pub async fn prep_swap(
       blockchain: &Blockchain, registry: &TokenRegistry,
       from: &str, to: &str, amount: f32, debug: bool) -> ErrStr<KyberSwap> {
    debug!("prep_swap", debug);

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
    let sned = http_client()?
        .get(&url)
        .query(&query)
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", &agent)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("KyberSwap route request failed: {e}"))?;

    // 1. Get raw text so it's not consumed if parsing fails
    let text = sned.text()
        .await
        .map_err(|e| format!("Failed to get text body: {}", e))?;

    // 2. Deserialize explicitly to catch the exact field error location
    let res: KyberResponse = serde_json::from_str(&text)
        .map_err(|e| {
            // This will print line/column and exact field mismatch details!
            format!("Serde parse error: {}.
Raw Body chunk: {}", e, &text[..text.len().min(500)])
        })?;

    let dat = res.data;

    // 4. Extract amount_out from the strongly-typed route_summary field
    let amount_out_str = dat.route_summary.get("amountOut")
        .and_then(|v| v.as_str()).ok_or(s("Response missing amountOut"))?;

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

async fn execute_trade_continuation(blockchain: &Blockchain, addy: &str,
                                    keystore_path: &str,
                                    registry: &TokenRegistry, from: &str,
                                    to: &str, amount: f32,
                                    slippage_bps: u16, debug: bool)
      -> ErrStr<(String, f32)> {
    debug!("execute_trade_continuation", debug);
    let signer = load_signer(blockchain, addy, keystore_path).await?;
    let provider = err_or(Provider::<Http>::try_from(blockchain.url()),
                          "Could not create RPC provider")?;
    let client = SignerMiddleware::new(provider, signer);
    let from_entry = registry.token(from)?;
    let amount_base =
       (amount * 10f32.powi(from_entry.decimals as i32)).round() as u128;
    log!(">>> Approving exact amount ({:.6} {}) for the router", amount, from);

    let prep = prep_swap(blockchain, registry, from, to, amount, debug).await?;

    let (approve_gas, mb_amt) = if from_entry.native {
       log!("Native swap; no ERC20 approval needed");
       (0.0, Some(U256::from(amount_base)))
    } else {
       let from_addr =
          from_entry.address.ok_or(format!("No address for {from}"))?;
       (approve_exact_amount(&client, &from_addr, &prep.router_address,
                            amount_base, debug).await?, None)
    };
    log!(">>> Requesting swap calldata from KyberSwap...");
    let (router, calldata) =
        kyberswap_build(blockchain, &prep.route_summary_raw, addy,
                        slippage_bps, debug).await?;
    log!(">>> Sending swap transaction...");
    let (tx_hash, swap_gas) =
       send_swap_tx(&client, &router, &calldata, mb_amt, debug).await?;

    Ok((tx_hash, approve_gas + swap_gas))
}

//============================================================================
//----- TESTS -------------------------------------------------------------
//============================================================================

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::create_testing;

   create_testing!("implementation::transactions::traders");

   run!("slippage", " (0.78 ETH at 200 BPS)", {
       let floor = 0.78;
       let adjusted = slippage_adjusted_floor(floor, 200);
       println!("adjusted floor at 200 BPS for 0.78 ETH is {adjusted:.4}");
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
    use super::*;

    #[test]
    fn test_slippage_adjusted_floor_raises_the_bar_by_the_tolerance() {
       // 200 bps = 2% tolerance: a quote must clear floor/0.98 so that
       // even a 2%-worse settlement still lands at or above floor.
       let floor = 500_000.0;
       let adjusted = slippage_adjusted_floor(floor, 200);
       assert!((adjusted - 500_000.0 / 0.98).abs() < 1e-6);
       // The tvá loss this fixes: a quote of 500,700 (0.14% above floor)
       // used to clear a raw 500,000 floor check, then settled 0.29%
       // under floor at 2% slippage. It must NOT clear the adjusted floor.
       assert!(500_700.0 <= adjusted,
          "a quote only 0.14% above floor must not clear a 2%-tolerance floor");
    }

    #[test]
    fn test_slippage_adjusted_floor_is_a_noop_at_zero_slippage() {
       assert_eq!(slippage_adjusted_floor(500_000.0, 0), 500_000.0);
    }

// TODO: I don't have tests for trades
}
