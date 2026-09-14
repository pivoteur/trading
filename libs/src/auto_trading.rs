use std::{
   collections::HashMap,
   path::Path,
   str::FromStr
};
use serde::Deserialize;
use serde_json::{ Value, from_str, json };

use book::{
    debug,
    currency::usd::{ USD, mk_usd },
    err_utils::{ ErrStr, err_or },
    file_utils::lines_from_file
};
use ethers::{
   middleware::SignerMiddleware,
   providers::{Http, Middleware, Provider},
   signers::{LocalWallet, Signer},
   types::{
      transaction::eip2718::TypedTransaction,
      Address, Bytes, Eip1559TransactionRequest, U256
   }
};
use libs::types::{ blockchains:: Blockchain, util::Id };

use super::{
   clients::http_client,
   hex::pad_address_for_call,
   logging::parse_log_ts,
   types::{
      balances::BalanceSnapshot,
      stats::CumulativeStats,
      tokens::{ TokenRegistry, TokenEntry }
   },
   wallets::wallet_balance
};

//============================================================================
//----- Shared Trading Constants -----------------------------------------------
//============================================================================

pub const UNDEAD: &str = "UNDEAD";
pub const NO_REAL_FLOOR: f64 = 0.000_000_01;

//============================================================================
//----- Live KyberSwap Quote --------------------------------------------------
//============================================================================

/// A live quote plus everything needed to actually build and sign the swap
/// afterward.
#[derive(Debug)]
pub struct KyberSwap {
    pub amount_out:         f64,
    pub route_summary_raw:  serde_json::Value,
    pub router_address:     String,
}

fn api_url(blockchain: &Blockchain) -> String {
    let base_url = "https://aggregator-api.kyberswap.com";
    format!("{base_url}/{}/api/v1", blockchain.blockchain())
}

pub async fn query_quote(blockchain: &Blockchain, registry: &TokenRegistry,
                         tok: &str, debug: bool) -> ErrStr<USD> {
   let kyb = query_swap(blockchain, registry, tok, "USDC", 1.0, debug).await?;
   Ok(mk_usd(kyb.amount_out as f32))
}

pub async fn query_swap(blockchain: &Blockchain, registry: &TokenRegistry,
                        from: &str, to: &str, amount: f64, debug: bool)
      -> ErrStr<KyberSwap> {
    debug!("query_swap", debug);
    let from_entry = registry.token(from)?;
    let to_entry = registry.token(to)?;
    fn addy(tok: &str, entry: &TokenEntry) -> ErrStr<String> {
       entry.address.clone().ok_or(format!("No address for token {tok}"))
    }
    let token_in = addy(from, &from_entry)?;
    let token_out = addy(to, &to_entry)?;
    let amount_in_base =
       (amount * 10f64.powi(from_entry.decimals as i32)).round() as u128;

    fn tok(dir: &str, token: &str) -> String { format!("token{dir}={token}") }
    let url = format!("{}/routes?{}&{}&amountIn={}", api_url(blockchain),
                      tok("In", &token_in), tok("Out", &token_out),
                      amount_in_base);

    log!("I am calling kyber...");
    let resp = err_or(http_client()?
        .get(&url)
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .header("Accept", "application/json")
        .send()
        .await,
        "KyberSwap route request failed")?;
    let status = resp.status();
    log!("kyber call completed: HTTP {}", status);
    let raw_body = err_or(resp.text().await,
                          "Could not read KyberSwap response body")?;
    let parsed: Value = err_or(from_str(&raw_body),
        &format!("KyberSwap response did not parse (HTTP {status})
Raw body: {raw_body}"))?;
    let data = parsed
        .get("data")
        .ok_or(format!("KyberSwap returned no route ({from} -> {to}). Raw: {raw_body}"))?;
    let route_summary_raw = data
        .get("routeSummary")
        .cloned()
        .ok_or(format!("Response missing routeSummary. Raw: {raw_body}"))?;
    let router_address = data
        .get("routerAddress")
        .and_then(|v| v.as_str())
        .ok_or(format!("Response missing routerAddress. Raw: {raw_body}"))?
        .to_string();
    let amount_out_str = route_summary_raw
        .get("amountOut")
        .and_then(|v| v.as_str())
        .ok_or(format!("routeSummary missing amountOut. Raw: {raw_body}"))?;
    let raw: u128 = err_or(amount_out_str.parse(),
        &format!("Could not parse amountOut '{amount_out_str}'"))?;
    let amount_out = raw as f64 / 10f64.powi(to_entry.decimals as i32);

    Ok(KyberSwap { amount_out, route_summary_raw, router_address })
}

//============================================================================
//----- Shared Pivot & Trade-Cycle Types ---------------------------------------
//============================================================================
#[derive(Debug, Clone, Deserialize)]
pub struct OpenPivot {
    pub pivot_id:      Id,
    pub opened_at:     u64,
    pub prim:          String,
    pub prim_amount:   f64,
    pub proper:        String,
    pub proper_amount: f64,
}

pub async fn pool_balance(blockchain: &Blockchain, addy: &str,
                          registry: &TokenRegistry, prim: &str,
                          committed: f64, undead_committed: f64)
      -> ErrStr<BalanceSnapshot> {
    // Two independent reads
    let asset_balance =
       wallet_balance(blockchain, addy, prim, registry).await?;
    let undead_balance =
       wallet_balance(blockchain, addy, UNDEAD, registry).await?;
    Ok(BalanceSnapshot {
        asset_balance,
        asset_committed: committed,
        asset_available: asset_balance - committed,
        undead_balance,
        undead_committed,
        undead_available: undead_balance - undead_committed,
    })
}

// "Biggest position first" — every survey/cycle closes its largest
/// commitments before its smallest.
pub fn biggest_first(mut pivots: Vec<OpenPivot>) -> Vec<OpenPivot> {
    pivots.sort_by(|a, b| {
        b.proper_amount.partial_cmp(&a.proper_amount).unwrap_or(std::cmp::Ordering::Equal)
    });
    pivots
}

//----- Misfire Reporting ------------------------------------------------------

/// Which half of a cycle a misfire happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisfireStage {
    Open,
    Close,
}

impl MisfireStage {
    fn label(self) -> &'static str {
        match self {
            MisfireStage::Open => "OPEN",
            MisfireStage::Close => "CLOSE",
        }
    }
}

/// Guesses a misfire's likely cause from the error text, loose substring match.
fn classify_misfire(err: &str) -> (&'static str, &'static str) {
    let e = err.to_lowercase();
    if e.contains("quote moved below your floor") {
        (
            "price moved between quoting and execution, past slippage tolerance",
            "no funds moved. If this repeats on this pair, slippage tolerance may be too tight.",
        )
    } else if e.contains("could not decrypt keystore") {
        (
            "keystore could not be decrypted with the password given",
            "check the keystore path and password secret match this wallet.",
        )
    } else if e.contains("does not match expected address") {
        (
            "keystore decrypted fine but belongs to a different wallet",
            "check the keystore wired into this run is really this instance's own.",
        )
    } else if e.contains("reverted on-chain") {
        (
            "transaction was accepted but reverted on execution",
            "check the tx hash on snowtrace.io -- usually a stale route or allowance mismatch.",
        )
    } else if e.contains("dropped or replaced") {
        (
            "transaction never confirmed -- dropped or replaced",
            "no funds spent; usually a gas-price race. Safe to let the next cycle retry.",
        )
    } else if e.contains("kyberswap") || e.contains("routesummary") || e.contains("router") {
        (
            "the KyberSwap quote/build request itself failed",
            "usually transient. If it repeats every cycle, check KyberSwap's API status.",
        )
    } else if e.contains("rpc") {
        (
            "the Avalanche RPC endpoint didn't answer cleanly",
            "usually transient. If it repeats every cycle, the RPC endpoint may be degraded.",
        )
    } else if e.contains("no tokens.toml entry") || e.contains("missing address") {
        (
            "a token this trade needed isn't registered correctly in tokens.toml",
            "check the token's entry in tokens.toml -- a config problem, will fail every cycle.",
        )
    } else {
        (
            "not a recognized failure shape -- see the raw error below",
            "read the raw error text below.",
        )
    }
}

/// Prints a misfire to stdout with a likely cause, not just the raw error.
pub fn report_misfire(stage: MisfireStage, pivot_id: Option<Id>, from: &str, to: &str, amount: f64, err: &str) {
    let (why, how) = classify_misfire(err);
    let pivot_label = pivot_id.map(|id| format!("pivot #{id}")).unwrap_or_else(|| "new position".to_string());

    println!(
        "  ! MISFIRE [{}] {pivot_label} {from}->{to} (attempted {amount:.8} {from}): {err}",
        stage.label()
    );
    println!("      likely cause: {why} -- {how}");
}

pub fn replay_log(path: &str) -> ErrStr<(Vec<OpenPivot>, Id, Id, CumulativeStats)> {
    if !Path::new(path).exists() {
        return Ok((Vec::new(), 1, 1, CumulativeStats::default()));
    }

    let lines = lines_from_file(path)?;

    let mut open_by_id: HashMap<Id, OpenPivot> = HashMap::new();
    let mut max_pivot_id: Id = 0;
    let mut max_close_id: Id = 0;
    let mut stats = CumulativeStats::default();

    for (line_no, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("timestamp\t") {
            continue; // blank, comment, or a header row
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let kind = *fields.get(1).unwrap_or(&"");

        if kind == "CHECK" {
            continue; // old-format tvá rows, tolerated but not counted
        }

        if fields.len() < 14 {
            return Err(format!("malformed line at {path}:{}: too few columns: '{line}'", line_no + 1));
        }
        let ts: u64 = parse_log_ts(fields[0])
            .map_err(|e| format!("{e} at {path}:{}: '{line}'", line_no + 1))?;
        let pivot_id_field = fields[2];
        let opened_pivot_id_field = fields[4];
        let prim = fields[5];
        let proper = fields[6];
        let prim_amount: f64 = fields[7].parse()
            .map_err(|_| format!("bad prim_amount at {path}:{}: '{line}'", line_no + 1))?;
        let proper_amount: f64 = fields[8].parse()
            .map_err(|_| format!("bad proper_amount at {path}:{}: '{line}'", line_no + 1))?;
        let gas_avax: f64 = fields[12].parse()
            .map_err(|_| format!("bad gas_avax at {path}:{}: '{line}'", line_no + 1))?;

        match kind {
            "OPEN" => {
                if !opened_pivot_id_field.is_empty() {
                    return Err(format!("OPEN at {path}:{} has a non-blank opened_pivot_id ('{opened_pivot_id_field}'): '{line}'", line_no + 1));
                }
                let pivot_id: Id = pivot_id_field.parse()
                    .map_err(|_| format!("bad pivot_id at {path}:{}: '{line}'", line_no + 1))?;
                max_pivot_id = max_pivot_id.max(pivot_id);
                stats.total_opens += 1;
                stats.total_gas_avax += gas_avax;
                open_by_id.insert(pivot_id, OpenPivot {
                    pivot_id, opened_at: ts,
                    prim: prim.to_string(), prim_amount,
                    proper: proper.to_string(), proper_amount,
                });
            }
            "CLOSE" => {
                if !pivot_id_field.is_empty() {
                    return Err(format!("CLOSE at {path}:{} has a non-blank pivot_id ('{pivot_id_field}') — closes don't open a pivot, use opened_pivot_id: '{line}'", line_no + 1));
                }
                let close_id: Id = fields[3].parse()
                    .map_err(|_| format!("bad close_id at {path}:{}: '{line}'", line_no + 1))?;
                let opened_pivot_id: Id = opened_pivot_id_field.parse()
                    .map_err(|_| format!("bad opened_pivot_id at {path}:{}: '{line}'", line_no + 1))?;
                let gain: f64 = fields[9].parse()
                    .map_err(|_| format!("bad gain at {path}:{}: '{line}'", line_no + 1))?;
                let roi: f64 = fields[10].parse()
                    .map_err(|_| format!("bad roi at {path}:{}: '{line}'", line_no + 1))?;
                let apr: f64 = fields[11].parse()
                    .map_err(|_| format!("bad apr at {path}:{}: '{line}'", line_no + 1))?;

                let closed_pivot = open_by_id.remove(&opened_pivot_id)
                    .ok_or_else(|| format!("CLOSE at {path}:{} references pivot #{opened_pivot_id}, which has no matching OPEN before it", line_no + 1))?;

                max_close_id = max_close_id.max(close_id);
                stats.total_closes += 1;
                stats.total_gas_avax += gas_avax;
                stats.roi_sum += roi;
                stats.apr_sum += apr;
                if closed_pivot.prim == UNDEAD {
                    stats.total_gain_undead += gain;
                } else {
                    stats.total_gain_asset += gain;
                }
            }
            "MISFIRE" => {
                if !pivot_id_field.is_empty() || !opened_pivot_id_field.is_empty() {
                    return Err(format!("MISFIRE at {path}:{} must leave pivot_id and opened_pivot_id blank: '{line}'", line_no + 1));
                }
                stats.total_gas_avax += gas_avax;
                if !fields[9].is_empty() {
                    let gain: f64 = fields[9].parse()
                        .map_err(|_| format!("bad gain at {path}:{}: '{line}'", line_no + 1))?;
                    if prim == UNDEAD {
                        stats.total_gain_undead += gain;
                    } else {
                        stats.total_gain_asset += gain;
                    }
                }
            }
            other => return Err(format!("unrecognized log line type '{other}' at {path}:{}: '{line}'", line_no + 1)),
        }
    }

    let still_open: Vec<OpenPivot> = open_by_id.into_values().collect();
    Ok((still_open, max_pivot_id + 1, max_close_id + 1, stats))
}

//============================================================================
//----- Shared Trade-Attempt Helper --------------------------------------------
//============================================================================
#[derive(Debug)]
pub enum AttemptOutcome {
    NotCleared,
    DryRunWouldClear { quoted_amount_out: f64 },
    Executed { tx_hash: String, actual_received: f64, gas_avax: f64 },
}

fn slippage_adjusted_floor(min_floor: f64, slippage_bps: u16) -> f64 {
    min_floor / (1.0 - slippage_bps as f64 / 10_000.0)
}

/// Quotes, checks the floor, and (unless dry-running) executes — the one
/// trade-attempt pipeline every pivot open/close in this system goes
/// through, regardless of which binary is calling it.
pub async fn attempt_trade_with_actual_amount(blockchain: &Blockchain,
     addy: &str, registry: &TokenRegistry, from: &str, to: &str, amount: f64,
     min_floor: f64, slippage_bps: u16, keystore_path: &str,
     dry_run: bool, debug: bool) -> ErrStr<AttemptOutcome> {
    let guaranteed_floor = slippage_adjusted_floor(min_floor, slippage_bps);
    let swap = query_swap(blockchain, registry, from, to, amount, debug).await?;
    if swap.amount_out <= guaranteed_floor {
            debug_trade_result(None, "NOT CLEARED", from, to, amount, &swap,
                               min_floor, debug);
        Ok(AttemptOutcome::NotCleared)
    } else {
        if dry_run {
           debug_trade_result(None, "DRY-RUN WOULD CLEAR", from, to, amount,
                              &swap, min_floor, debug);
           Ok(AttemptOutcome::DryRunWouldClear {
              quoted_amount_out: swap.amount_out
           })
        } else {
            let balance_before =
               wallet_balance(blockchain, addy, to, registry).await?;
            let (tx_hash, gas_avax) =
               execute_trade(blockchain, addy, registry, from, to, amount,
                             min_floor, slippage_bps, keystore_path,
                             debug).await?;
            let balance_after =
               wallet_balance(blockchain, addy, to, registry).await?;
            let actual_received = balance_after - balance_before;
                debug_trade_result(Some(&tx_hash), "EXECUTED", from, to,
                                   amount, &swap, min_floor, debug);
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax })
        }
    }
}

fn debug_trade_result(tx: Option<&str>, kind: &str, from: &str, to: &str,
                      amount: f64, swap: &KyberSwap, min_floor: f64,
                      debug: bool) {
   debug!("attempt_trade_with_actual_amount", debug);
   let trade = format!("Trade {from} -> {to}");
   let mb_tx = tx.and_then(|t| Some(format!("tx {t}"))).unwrap_or_default();
   let swap = format!("swap {amount:.4} -> {:.4}", swap.amount_out);
   let floor = format!("(floor {min_floor:.4})");
   let line = format!("{trade} {mb_tx} {swap} {floor}");
   log!("[{}] {}", kind, line);
}

//============================================================================
//----- Signing & Execution ----------------------------------------------------
//============================================================================
// Everything past this point can move real funds. Every function here is
// deliberately loud on failure.

fn pad_u256_for_call(amount: u128) -> String { format!("{amount:064x}") }

/// AVAX cost of a confirmed transaction, computed from its own receipt
/// (gas_used * effective_gas_price), not estimated beforehand. If a
/// receipt is somehow missing pricing info, returns 0.0 rather than
/// failing the whole trade over a cosmetic figure — the trade itself
/// already succeeded by the time this is called.
fn gas_cost_avax(gas_used: Option<U256>, gas_price: Option<U256>) -> f64 {
    match (gas_used, gas_price) {
        (Some(g), Some(p)) => {
            let wei = g.saturating_mul(p);
            wei.as_u128() as f64 / 1e18
        }
        _ => 0.0,
    }
}

pub async fn load_signer(blockchain: &Blockchain, expected_address: &str,
                         keystore_path: &str) -> ErrStr<LocalWallet> {
    let password = match std::env::var("KEYSTORE_PASSWORD") {
        Ok(pw) => pw,
        Err(_) => err_or(rpassword::prompt_password("Keystore password: "),
                         "Could not read password. No funds moved.")?
    };
    let wallet =
       err_or(LocalWallet::decrypt_keystore(&keystore_path, &password),
              &format!("Could not decrypt keystore, path {keystore_path}.
No funds moved."))?
             .with_chain_id(blockchain.chain_id());
    let derived = format!("{:?}", wallet.address());
    if !derived.eq_ignore_ascii_case(expected_address) {
        Err(format!("Keystore address ({derived}) does not match expected
address ({expected_address}) — refusing to proceed. No funds moved."
        ))
    } else {
       Ok(wallet)
    }
}

/// Builds an EIP-1559 tx with a buffered max fee (so a base-fee bump between
/// estimation and submission — e.g. during the password prompt — doesn't
/// get the tx rejected pre-mempool) and a buffered gas limit. Fees are
/// re-estimated fresh on every call rather than reused across steps.
async fn build_tx_with_fee_buffer(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>, to: Address,
        data: Bytes) -> ErrStr<Eip1559TransactionRequest> {
    let (max_fee, max_priority_fee) =
       err_or(client.estimate_eip1559_fees(None).await,
              "Could not estimate EIP-1559 fees")?;
    // 30% buffer on the max fee absorbs a base-fee bump between estimation
    // and submission without overpaying on the priority fee.
    let buffered_max_fee =
       max_fee.saturating_mul(U256::from(130)) / U256::from(100);
    let tx = Eip1559TransactionRequest::new()
        .to(to)
        .data(data)
        .max_fee_per_gas(buffered_max_fee)
        .max_priority_fee_per_gas(max_priority_fee);

    let typed: TypedTransaction = tx.clone().into();
    let gas_estimate = err_or(client.estimate_gas(&typed, None).await,
                              "Could not estimate gas limit")?;
    // 20% buffer on gas so slightly-off estimate doesn't run out mid-execution.
    let buffered_gas =
       gas_estimate.saturating_mul(U256::from(120)) / U256::from(100);
    Ok(tx.gas(buffered_gas))
}

/// Approves the router for EXACTLY this trade's amount — never a standing
/// allowance. The router can never pull more than what's approved here.
pub async fn approve_exact_amount(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>,
        token_contract: &str,
        spender: &str,
        amount_base_units: u128,
        verbose: bool) -> ErrStr<f64> {
    debug!("approve_exact_amount", verbose);

    let data_hex = format!(
        "0x095ea7b3{}{}",
        pad_address_for_call(spender),
        pad_u256_for_call(amount_base_units)
    );
    let to = err_or(Address::from_str(token_contract), "Bad token address")?;
    let data = err_or(Bytes::from_str(&data_hex), "Bad approve calldata")?;
    let tx = build_tx_with_fee_buffer(client, to, data).await?;
    let (_tx, gas) =
       complete_transaction(client, tx, "approve", false, verbose).await?;
    Ok(gas)
}

/// Asks KyberSwap to encode the actual swap calldata for the route.
/// When `verbose`, prints the raw response so it can be eyeballed before
/// trusting it — otherwise that's a screen-filling blob of hex calldata,
/// so it stays silent by default. `slippage_bps` is basis points (e.g.
/// 50 = 0.50%).
pub async fn kyberswap_build(blockchain: &Blockchain, route_summary_raw: &Value,
                             sender: &str, slippage_bps: u16, verbose: bool)
      -> ErrStr<(String, String)> {
    debug!("kyberswap_build", verbose);
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
        router: &str,
        calldata_hex: &str,
        verbose: bool) -> ErrStr<(String, f64)> {
    let to = err_or(Address::from_str(router), "Bad router address")?;
    let data =
         err_or(Bytes::from_str(calldata_hex), "Bad calldata from KyberSwap")?;
    let tx = build_tx_with_fee_buffer(client, to, data).await?;
    complete_transaction(client, tx, "swap", true, verbose).await
}

async fn complete_transaction(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>,
        tx: Eip1559TransactionRequest, tx_type: &str, check_state: bool,
        verbose: bool) -> ErrStr<(String, f64)> {
    debug!("complete_transaction", verbose);
    let pending = err_or(client.send_transaction(tx, None).await,
                         &format!("{tx_type} transaction failed to send"))?;
    let tx_hash = format!("{:?}", pending.tx_hash());
    log!("Transaction {} tx submitted: {}", tx_type, tx_hash);

    let receipt =
       err_or(pending.await,
              &format!("{tx_type} transaction failed while confirming"))?;
    match receipt {
        Some(r) => {
            if !check_state || r.status == Some(1.into()) {
               log!("Transaction {} confirmed in block {}",
                    tx_type, format!("{:?}", r.block_number));
               Ok((tx_hash, gas_cost_avax(r.gas_used, r.effective_gas_price)))
            } else {
               Err(format!("{tx_type} transaction REVERTED on-chain.
Hash: {tx_hash}"))
            }
        }
        None => Err(format!("{tx_type} transaction was dropped or replaced.
Hash: {tx_hash}"))
    }
}

/// Transfer core behind `send_tokens_to_address` — a plain ERC-20
/// transfer(), not a swap. Same safety baseline as execute_trade: verified
/// signer, EIP-1559 fee buffering, hard error on revert/drop/failure
/// rather than a false success. Does not support native AVAX (no
/// tokens.toml address to encode against) — only ERC-20s with a real
/// contract address. `to_address` is always a literal here — nothing in
/// this file resolves an address from env internally anymore.
pub async fn send_tokens_to_address(blockchain: &Blockchain,
                                    addy: &str, registry: &TokenRegistry,
                                    symbol: &str, to_address: &str,
                                    amount: f64, keystore_path: &str,
                                    verbose: bool) -> ErrStr<(String, f64)> {
    if amount <= 0.0 {
        Err(format!("amount must be positive, got {amount}"))
    } else {
        send_tokens(blockchain, addy, registry, symbol, to_address, amount,
                    keystore_path, verbose).await
    }
}

async fn send_tokens(blockchain: &Blockchain,
                     addy: &str, registry: &TokenRegistry, symbol: &str,
                     to_address: &str, amount: f64, keystore_path: &str,
                     verbose: bool) -> ErrStr<(String, f64)> {
    debug!("send_tokens_to_address", verbose);
    let signer = load_signer(blockchain, addy, keystore_path).await?;
    let provider = err_or(Provider::<Http>::try_from(&blockchain.url()),
                          "Could not create RPC provider")?;
    let client = SignerMiddleware::new(provider, signer);
    let entry = registry.token(symbol)?;
    let token_addr = entry.address.ok_or(format!("No address for {symbol}"))?;
    let amount_base =
       (amount * 10f64.powi(entry.decimals as i32)).round() as u128;

    // transfer(address,uint256) selector = 0xa9059cbb
    let data_hex = format!(
        "0xa9059cbb{}{}",
        pad_address_for_call(to_address),
        pad_u256_for_call(amount_base)
    );
    let to = err_or(Address::from_str(&token_addr), "Bad token address")?;
    let data = err_or(Bytes::from_str(&data_hex), "Bad transfer calldata")?;
    let tx = build_tx_with_fee_buffer(&client, to, data).await?;
    log!("On its way — courier's en route...");
    complete_transaction(&client, tx, "transfer", true, verbose).await
}

pub async fn execute_trade(blockchain: &Blockchain, addy: &str,
                           registry: &TokenRegistry, from: &str, to: &str,
                           amount: f64, min_floor: f64, slippage_bps: u16,
                           keystore_path: &str, verbose: bool)
      -> ErrStr<(String, f64)> {
    debug!("execute_trade", verbose);
    log!("Trading in progress — approve/quote/swap. This part takes a minute.");
    log!(">>> Re-checking the quote after keystore unlock (it may have moved)");
    let fresh_quote =
       query_swap(blockchain, registry, from, to, amount, verbose).await?;
    let ratio = fresh_quote.amount_out;
    let trade = format!("{amount:.6} {from} -> {:.8} {to}", ratio);
    log!("Fresh quote: {}", trade);

    // See slippage_adjusted_floor: the swap below is authorized (via
    // slippage_bps) to settle as low as fresh_quote * (1 - slippage_bps),
    // so the fresh quote itself must clear that worse case, not just
    // min_floor, or a real close can settle under floor.
    
    let guaranteed_floor = slippage_adjusted_floor(min_floor, slippage_bps);
    if fresh_quote.amount_out < guaranteed_floor {
       Err(format!("Quote moved below your floor while unlocking the keystore
({:.8} {to} quoted, but only {:.8} {to} is guaranteed at {slippage_bps} bps
slippage tolerance -- need > {min_floor:.8} {to}).
        
That's not happening. No funds used.", ratio,
           ratio * (1.0 - slippage_bps as f64 / 10_000.0)
        )) 
    } else { 
       execute_trade_continuation(blockchain, addy, keystore_path,
                                  registry, from, amount, fresh_quote,
                                  slippage_bps, verbose).await
    }
}

async fn execute_trade_continuation(blockchain: &Blockchain, addy: &str,
                                    keystore_path: &str,
                                    registry: &TokenRegistry, from: &str,
                                    amount: f64, fresh_quote: KyberSwap,
                                    slippage_bps: u16, verbose: bool)
      -> ErrStr<(String, f64)> {
    debug!("execute_trade_continuation", verbose);
    let signer = load_signer(blockchain, addy, keystore_path).await?;
    let provider = err_or(Provider::<Http>::try_from(blockchain.url()),
                          "Could not create RPC provider")?;
    let client = SignerMiddleware::new(provider, signer);
    let from_entry = registry.token(from)?;
    let from_addr = from_entry.address.ok_or(format!("No address for {from}"))?;
    let amount_base =
       (amount * 10f64.powi(from_entry.decimals as i32)).round() as u128;
    log!(">>> Approving exact amount ({:.6} {}) for the router", amount, from);
    let approve_gas =
       approve_exact_amount(&client, &from_addr, &fresh_quote.router_address,
                            amount_base, verbose).await?;
    log!(">>> Requesting swap calldata from KyberSwap...");
    let (router, calldata) =
        kyberswap_build(blockchain, &fresh_quote.route_summary_raw, addy,
                        slippage_bps, verbose).await?;
    log!(">>> Sending swap transaction...");
    let (tx_hash, swap_gas) =
       send_swap_tx(&client, &router, &calldata, verbose).await?;

    Ok((tx_hash, approve_gas + swap_gas))
}


//============================================================================
//----- UNIT TESTS -------------------------------------------------------------
//============================================================================

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };
   use crate::fetchers::tokens::fetch_tokens;

   create_testing!("auto_trading");

   async fn quote_for(tok: &str) -> ErrStr<()> {
      let token = tok.to_uppercase();
      let ava = &Blockchain::AVALANCHE;
      let reg = fetch_tokens(ava).await?;
      let quote = query_quote(ava, &reg, &token, true).await?;
      println!("{token} quote: {quote}");
      Ok(())
   }

   run!("btc_quote", now(quote_for("btc"))?);
   run!("undead_quote", now(quote_for("undead"))?);
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod unit_tests {
    use super::*;
    use crate::{
       fetchers::tokens::fetch_tokens,
       logging::{ log_misfire, log_row }
    };
    use libs::types::blockchains::Blockchain::AVALANCHE;

   #[tokio::test] async fn test_query_swap() -> ErrStr<()> {
      let blockchain = &AVALANCHE;
      let tokens = fetch_tokens(blockchain).await?;
      let query =
         query_swap(blockchain, &tokens, "BTC", "ETH", 1.0, true).await;
      assert!(query.is_ok());
      Ok(())
   }

   #[tokio::test] async fn test_query_swap_btc_eth_ratio() -> ErrStr<()> {
      let blockchain = &AVALANCHE;
      let tokens = fetch_tokens(blockchain).await?;
      let query =
         query_swap(blockchain, &tokens, "BTC", "ETH", 1.0, true).await?;
      let ratio = query.amount_out;
      assert!(ratio > 16.0, "The ratio BTC/ETH is {ratio}");
      Ok(())
   }

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

    #[test]
    fn test_biggest_first_sorts_by_raw_proper_amount_descending() {
        let make = |id: Id, proper_amount: f64| OpenPivot {
            pivot_id: id, opened_at: 0, prim: "X".into(), prim_amount: 0.0,
            proper: "Y".into(), proper_amount,
        };
        let pivots = vec![make(1, 5e5), make(2, 0.005), make(3, 5.2e5)];
        let sorted = biggest_first(pivots);
        let ids: Vec<Id> = sorted.iter().map(|p| p.pivot_id).collect();
        assert_eq!(ids, vec![3, 1, 2],
                   "should be ordered biggest proper_amount to smallest,
raw number, no currency conversion");
    }

    #[test]
    fn test_replay_log_missing_file_replays_as_a_fresh_empty_pool()
          -> ErrStr<()> {
        let (opens, next_pivot, next_close, stats) =
           replay_log("/tmp/definitely_does_not_exist.log")?;
        assert!(opens.is_empty());
        assert_eq!(next_pivot, 1);
        assert_eq!(next_close, 1);
        assert_eq!(stats.total_opens, 0);
        Ok(())
    }

    #[test]
    fn test_replay_log_open_then_close_leaves_nothing_open_and_totals_gain() -> ErrStr<()> {
        let path = std::env::temp_dir().join("auto_trading_test_open_close.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n\
 1970-01-01 00:33:20\tCLOSE\t\t1\t1\tUNDEAD\tBTC\t500000.00000000\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, next_pivot, next_close, stats) = replay_log(path_str)?;
        assert!(opens.is_empty(), "pivot 1 was closed, should not appear as open");
        assert_eq!(next_pivot, 2);
        assert_eq!(next_close, 2);
        assert_eq!(stats.total_opens, 1);
        assert_eq!(stats.total_closes, 1);
        assert!((stats.total_gain_undead - 11112.13).abs() < 0.001, "gain should land in the UNDEAD bucket (prim was UNDEAD)");
        assert_eq!(stats.total_gain_asset, 0.0);
        assert!((stats.total_gas_avax - 0.008).abs() < 0.00001, "gas should sum across the OPEN and CLOSE");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn test_replay_log_open_without_close_stays_open() -> ErrStr<()> {
        let path = std::env::temp_dir().join("auto_trading_test_open_only.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n\
 1970-01-01 00:16:40\tOPEN\t2\t\t\tBTC\tUNDEAD\t0.00500000\t487122.54000000\t\t\t\t0.00300000\t0xdef\n\
 1970-01-01 00:33:20\tCHECK\t1\tnot_closed\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, next_pivot, next_close, stats) = replay_log(path_str)?;
        assert_eq!(opens.len(), 2, "neither pivot was closed, both should still be open");
        assert_eq!(next_pivot, 3);
        assert_eq!(next_close, 1, "no CLOSE lines yet, so next_close_id stays at 1");
        assert_eq!(stats.total_closes, 0, "old-format CHECK lines are tolerated but don't count as closes");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn test_replay_log_skips_a_leading_header_row() -> ErrStr<()> {
        let path = std::env::temp_dir().join("auto_trading_test_header.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "timestamp\tkind\tpivot_id\tclose_id\topened_pivot_id\tprim\tproper\tprim_amount\tproper_amount\tgain\troi\tapr\tgas_avax\ttx_hash\n\
 1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, _, _, _) = replay_log(path_str)?;
        assert_eq!(opens.len(), 1, "the header row should be skipped, not treated as a malformed data row");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn test_replay_log_rejects_malformed_line() {
        let path = std::env::temp_dir().join("auto_trading_test_malformed.log");
        std::fs::write(&path, "not\teven\tclose\tto\tvalid\n").unwrap();
        let result = replay_log(path.to_str().unwrap());
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_replay_log_rejects_close_with_no_matching_open() {
        let path = std::env::temp_dir().join("auto_trading_test_orphan_close.log");
        std::fs::write(
            &path,
            "2026-01-01 00:00:00\tCLOSE\t\t1\t99\tUNDEAD\tBTC\t500000.00000000\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
        ).unwrap();
        let result = replay_log(path.to_str().unwrap());
        assert!(result.is_err(), "a CLOSE referencing a pivot_id with no prior OPEN should be a hard error, not silently ignored");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_replay_log_misfire_does_not_create_an_open_pivot() -> ErrStr<()> {
        let path = std::env::temp_dir().join("auto_trading_test_misfire.log");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(&path); // clean slate -- log_misfire appends, it doesn't truncate
        let snap = BalanceSnapshot {
            asset_balance: 0.005, asset_committed: 0.0, asset_available: 0.005,
            undead_balance: 500_000.0, undead_committed: 0.0, undead_available: 500_000.0
        };
        let cum = CumulativeStats::default();
        log_misfire(path_str, None, "UNDEAD", "BTC", 500_000.0, 0.0, "",
                    &snap, &cum);

        let (opens, next_pivot, next_close, stats) = replay_log(path_str)?;
        assert!(opens.is_empty(),
                "a MISFIRE must never be replayed as a real open pivot");
        assert_eq!(next_pivot, 1,
                   "id counters must not advance from a MISFIRE");
        assert_eq!(next_close, 1);
        assert_eq!(stats.total_opens, 0);
        assert_eq!(stats.total_closes, 0);

        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn test_replay_log_rejects_misfire_with_a_pivot_id() {
        let path =
           std::env::temp_dir().join("auto_trading_test_misfire_bad.log");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(&path);
        let snap = BalanceSnapshot {
            asset_balance: 0.005, asset_committed: 0.0, asset_available: 0.005,
            undead_balance: 500_000.0, undead_committed: 0.0, undead_available: 500_000.0,
        };
        let cum = CumulativeStats::default();
        log_row(path_str, None, "MISFIRE", Some(1), None, None, "UNDEAD", "BTC", 500_000.0, 0.0, None, None, None, 0.0, "", &snap, &cum);

        let result = replay_log(path_str);
        assert!(result.is_err(), "a MISFIRE row must never carry a pivot_id -- that would make it indistinguishable from a real OPEN");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_classify_misfire_quote_moved_below_floor() {
        let (why, _how) = classify_misfire("Quote moved below your floor while unlocking the keystore (0.00490000 BTC quoted, but only 0.00485000 BTC is guaranteed at 50 bps slippage tolerance -- need > 0.00500000 BTC). That's not happening. No funds used.");
        assert!(why.contains("price moved"), "expected floor-slippage classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_keystore_decrypt_failure() {
        let (why, _how) = classify_misfire("Could not decrypt keystore, path /tmp/x.json: invalid password. No funds moved.");
        assert!(why.contains("could not be decrypted"), "expected keystore classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_wrong_wallet_address() {
        let (why, _how) = classify_misfire("Keystore address (0xabc) does not match expected address (0xdef) — refusing to proceed. No funds moved.");
        assert!(why.contains("different wallet"), "expected address-mismatch classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_reverted_on_chain() {
        let (why, _how) = classify_misfire("Swap transaction REVERTED on-chain. Hash: 0xabc123");
        assert!(why.contains("reverted"), "expected revert classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_dropped_or_replaced() {
        let (why, _how) = classify_misfire("Swap transaction was dropped or replaced. Hash: 0xabc123");
        assert!(why.contains("dropped or replaced"), "expected drop/replace classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_kyberswap_failure() {
        let (why, _how) = classify_misfire("KyberSwap route request failed: connection reset");
        assert!(why.contains("KyberSwap"), "expected KyberSwap classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_rpc_failure() {
        let (why, _how) = classify_misfire("RPC request (eth_call) failed: timed out");
        assert!(why.contains("RPC"), "expected RPC classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_missing_token_entry() {
        let (why, _how) = classify_misfire("No tokens.toml entry for 'FOO' — add one before checking this pool");
        assert!(why.contains("tokens.toml"), "expected tokens.toml classification, got: '{why}'");
    }

    #[test]
    fn test_classify_misfire_falls_back_on_unrecognized_error() {
        let (why, how) = classify_misfire("something totally unexpected happened");
        assert!(why.contains("not a recognized failure shape"), "expected fallback classification, got: '{why}'");
        assert!(how.contains("raw error"), "expected fallback hint to point at the raw error, got: '{how}'");
    }

    #[test]
    fn test_report_misfire_open_does_not_panic() {
        report_misfire(MisfireStage::Open, None, "BTC", "UNDEAD", 0.005, "some error");
    }

    #[test]
    fn test_report_misfire_close_does_not_panic() {
        report_misfire(MisfireStage::Close, Some(12), "UNDEAD", "BTC", 500_000.0, "Swap transaction REVERTED on-chain. Hash: 0xdef");
    }
}
