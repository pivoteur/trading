use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use book::{
        err_utils::ErrStr,
        file_utils::lines_from_file,
        string_utils::s,
        utils::get_env
};
use ethers::{
        middleware::SignerMiddleware,
        providers::{Http, Middleware, Provider},
        signers::{LocalWallet, Signer},
        types::{
            transaction::eip2718::TypedTransaction, Address, Bytes, Eip1559TransactionRequest, U256,
        },
};
use serde::Deserialize;

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TokenEntry {
    #[serde(default)]
    pub native:   bool,
    #[serde(default)]
    pub address:  Option<String>,
    pub decimals: u32,
}

pub type TokenRegistry = HashMap<String, TokenEntry>;

/// Each binary embeds its own `tokens.toml` via `include_str!` (the token
/// set differs per binary) and passes the raw string here to parse it.
pub fn parse_token_registry(toml_str: &str) -> ErrStr<TokenRegistry> {
    toml::from_str(toml_str).map_err(|e| format!("Failed to parse tokens.toml: {e}"))
}

pub fn token_entry<'a>(registry: &'a TokenRegistry, symbol: &str) -> ErrStr<&'a TokenEntry> {
    match registry.get(symbol) {
        Some(entry) => Ok(entry),
        None => Err(format!(
            "No tokens.toml entry for '{symbol}' — add one before checking this pool"
        )),
    }
}

//============================================================================
//----- Shared Trading Constants -----------------------------------------------
//============================================================================
/// Every asset<->UNDEAD pool binary in this codebase (tvá, arbitrage)
/// trades against UNDEAD as the fixed other leg.
pub const UNDEAD: &str = "UNDEAD";

/// Used for opening pivots, where there's no real floor to enforce yet —
/// a near-zero floor means "accept whatever the live quote is," while
/// still routing through the same clear-a-floor codepath as every other
/// trade, rather than a separate unchecked path. Shared by every binary
/// that opens fresh pivots (tvá, arbitrage).
pub const NO_REAL_FLOOR: f64 = 0.000_000_01;

//============================================================================
//----- Shared HTTP Client ----------------------------------------------------
//============================================================================
const HTTP_TIMEOUT_SECS: u64 = 15;

fn http_client() -> ErrStr<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Could not build HTTP client: {e}"))
}

//============================================================================
//----- Wallet Balance Check --------------------------------------------------
//============================================================================
pub const AVALANCHE_RPC: &str = "https://api.avax.network/ext/bc/C/rpc";
pub const AVALANCHE_CHAIN_ID: u64 = 43114;
const KYBERSWAP_CHAIN: &str = "avalanche";

pub fn wallet_address_from_env(var_name: &str) -> ErrStr<String> {
    std::env::var(var_name).map_err(|_| {
        let ans = format!("Missing required env var: {var_name} (your public wallet address)");
        ans
    })
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<String>,
    error:  Option<serde_json::Value>,
}

async fn rpc_call(method: &str, params: serde_json::Value) -> ErrStr<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let resp = http_client()?
        .post(AVALANCHE_RPC)
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
    parsed
        .result
        .ok_or_else(|| format!("RPC call {method} returned no result"))
}

fn hex_to_u128(hex: &str) -> ErrStr<u128> {
    let trimmed = hex.trim_start_matches("0x");
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    u128::from_str_radix(trimmed, 16)
        .map_err(|e| format!("Could not parse hex balance '{hex}': {e}"))
}

fn pad_address_for_call(address: &str) -> String {
    let hex = address.trim_start_matches("0x").to_lowercase();
    let ans = format!("{hex:0>64}");
    ans
}

async fn erc20_balance(wallet_address: &str, token_contract: &str) -> ErrStr<u128> {
    // balanceOf(address) selector = 0x70a08231
    let data = format!("0x70a08231{}", pad_address_for_call(wallet_address));
    let result = rpc_call(
        "eth_call",
        serde_json::json!([{ "to": token_contract, "data": data }, "latest"]),
    )
    .await?;
    hex_to_u128(&result)
}

/// Native-coin balance (AVAX itself, not a wrapped ERC-20) via the plain
/// `eth_getBalance` RPC method — no contract call, no `address` needed in
/// tokens.toml. `wallet_balance` below dispatches here automatically for
/// any registry entry marked `native = true`.
async fn native_coin_balance(wallet_address: &str) -> ErrStr<u128> {
    let result = rpc_call(
        "eth_getBalance",
        serde_json::json!([wallet_address, "latest"]),
    )
    .await?;
    hex_to_u128(&result)
}

pub async fn wallet_balance(
    wallet_address: &str,
    symbol: &str,
    registry: &TokenRegistry,
) -> ErrStr<f64> {
    let entry = token_entry(registry, symbol)?;
    let raw = if entry.native {
        native_coin_balance(wallet_address).await?
    } else {
        let addr = entry
            .address
            .as_deref()
            .ok_or_else(|| format!("'{symbol}' is not marked native and has no address in tokens.toml — add one or set native = true"))?;
        erc20_balance(wallet_address, addr).await?
    };
    Ok(raw as f64 / 10f64.powi(entry.decimals as i32))
}

//============================================================================
//----- Live KyberSwap Quote --------------------------------------------------
//============================================================================
/// A live quote plus everything needed to actually build and sign the swap
/// afterward.
pub struct KyberSwap {
    pub amount_out:         f64,
    pub route_summary_raw:  serde_json::Value,
    pub router_address:     String,
}

pub async fn kyber_swap(
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
) -> ErrStr<KyberSwap> {
    let from_entry = token_entry(registry, from_symbol)?;
    let to_entry = token_entry(registry, to_symbol)?;
    let token_in = from_entry.address.as_deref().ok_or_else(|| format!("{from_symbol} missing address"))?;
    let token_out = to_entry.address.as_deref().ok_or_else(|| format!("{to_symbol} missing address"))?;
    let amount_in_base = (amount * 10f64.powi(from_entry.decimals as i32)).round() as u128;

    let url = format!(
        "https://aggregator-api.kyberswap.com/{KYBERSWAP_CHAIN}/api/v1/routes?tokenIn={token_in}&tokenOut={token_out}&amountIn={amount_in_base}"
    );

    let resp = http_client()?
        .get(&url)
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("KyberSwap route request failed: {e}"))?;

    let status = resp.status();
    let raw_body = resp
        .text()
        .await
        .map_err(|e| format!("Could not read KyberSwap response body: {e}"))?;

    let parsed: serde_json::Value = serde_json::from_str(&raw_body).map_err(|e| {
        format!("KyberSwap response did not parse (HTTP {status}): {e}\nRaw body: {raw_body}")
    })?;

    let data = parsed
        .get("data")
        .ok_or_else(|| format!("KyberSwap returned no route ({from_symbol} -> {to_symbol}). Raw: {raw_body}"))?;
    let route_summary_raw = data
        .get("routeSummary")
        .cloned()
        .ok_or_else(|| format!("Response missing routeSummary. Raw: {raw_body}"))?;
    let router_address = data
        .get("routerAddress")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Response missing routerAddress. Raw: {raw_body}"))?
        .to_string();
    let amount_out_str = route_summary_raw
        .get("amountOut")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("routeSummary missing amountOut. Raw: {raw_body}"))?;
    let raw: u128 = amount_out_str
        .parse()
        .map_err(|_| format!("Could not parse amountOut '{amount_out_str}'"))?;
    let amount_out = raw as f64 / 10f64.powi(to_entry.decimals as i32);

    Ok(KyberSwap { amount_out, route_summary_raw, router_address })
}

//============================================================================
//----- Shared Pivot & Trade-Cycle Types ---------------------------------------
//============================================================================
// Everything in this section used to be duplicated, near byte-for-byte,
// between tvá's and arbitrage's mod.rs — the only differences were the
// name of the non-UNDEAD leg ("BTC" for tvá's one fixed pair, "TOKEN" for
// whichever pool arbitrage is working) and, for arbitrage, a `path` per
// pool instead of one fixed log file. Both are now plain parameters.

/// One open position: `proper` is what was spent to open it, `prim` is
/// what closing it will swap back to.
#[derive(Debug, Clone)]
pub struct OpenPivot {
    pub pivot_id:      u32,
    pub opened_at:     u64,
    pub prim:          String,
    pub prim_amount:   f64,
    pub proper:        String,
    pub proper_amount: f64,
}

/// Running totals across a pivot log's whole history. `total_gain_asset`
/// is the cumulative realized gain in whichever token isn't UNDEAD (BTC
/// for tvá, the pool's token for arbitrage); `total_gain_undead` is the
/// same for UNDEAD.
#[derive(Debug, Clone, Default)]
pub struct CumulativeStats {
    pub total_opens:       u32,
    pub total_closes:      u32,
    pub total_gain_asset:  f64,
    pub total_gain_undead: f64,
    pub total_gas_avax:    f64,
    pub roi_sum: f64,
    pub apr_sum: f64,
}

impl CumulativeStats {
    pub fn avg_roi(&self) -> f64 {
        if self.total_closes == 0 { 0.0 } else { self.roi_sum / self.total_closes as f64 }
    }
    pub fn avg_apr(&self) -> f64 {
        if self.total_closes == 0 { 0.0 } else { self.apr_sum / self.total_closes as f64 }
    }
}

/// Wallet balances for one asset<->UNDEAD pair — `asset` is BTC for tvá
/// (its one fixed pair) and whichever token a pool is keyed on for
/// arbitrage (one pair per pool).
#[derive(Debug, Clone, Copy)]
pub struct BalanceSnapshot {
    pub asset_balance:    f64,
    pub asset_committed:  f64,
    pub asset_available:  f64,
    pub undead_balance:   f64,
    pub undead_committed: f64,
    pub undead_available: f64,
}

pub async fn balance_snapshot(
    wallet_address: &str,
    registry: &TokenRegistry,
    asset_symbol: &str,
    asset_committed: f64,
    undead_committed: f64,
) -> ErrStr<BalanceSnapshot> {
    // Two independent reads — nothing here depends on the other's result —
    // so they run concurrently instead of one after another. This is only
    // safe because both sides are reads; the trade-execution path stays
    // sequential per wallet since signing needs nonces to land in order.
    let (asset_balance, undead_balance) = tokio::try_join!(
        wallet_balance(wallet_address, asset_symbol, registry),
        wallet_balance(wallet_address, UNDEAD, registry),
    )?;
    Ok(BalanceSnapshot {
        asset_balance,
        asset_committed,
        asset_available: asset_balance - asset_committed,
        undead_balance,
        undead_committed,
        undead_available: undead_balance - undead_committed,
    })
}

pub fn snapshot_and_cumulative_columns(snap: &BalanceSnapshot, cum: &CumulativeStats) -> String {
    let ans = format!(
        "{:.8}\t{:.8}\t{:.8}\t{:.2}\t{:.2}\t{:.2}\t{:+.8}\t{:+.2}\t{:.8}\t{:.6}\t{:.6}",
        snap.asset_balance, snap.asset_committed, snap.asset_available,
        snap.undead_balance, snap.undead_committed, snap.undead_available,
        cum.total_gain_asset, cum.total_gain_undead, cum.total_gas_avax,
        cum.avg_roi(), cum.avg_apr(),
    );
    ans
}

/// "Biggest position first" — every survey/cycle closes its largest
/// commitments before its smallest.
pub fn biggest_first(mut pivots: Vec<OpenPivot>) -> Vec<OpenPivot> {
    pivots.sort_by(|a, b| {
        b.proper_amount.partial_cmp(&a.proper_amount).unwrap_or(std::cmp::Ordering::Equal)
    });
    pivots
}

pub fn now_ts() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub const LOG_TS_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

pub fn log_ts(epoch: u64) -> String {
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.format(LOG_TS_FORMAT).to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

pub fn parse_log_ts(s: &str) -> ErrStr<u64> {
    chrono::NaiveDateTime::parse_from_str(s, LOG_TS_FORMAT)
        .map(|ndt| ndt.and_utc().timestamp() as u64)
        .map_err(|e| format!("bad timestamp '{s}' (expected UTC '{LOG_TS_FORMAT}', e.g. '2026-08-05 14:32:07'): {e}"))
}

/// Appends one line to a trade log, creating the file on first write.
/// `header` is `None` for logs that never had one (tvá's single fixed
/// log) and `Some(..)` for logs that write a header the first time they're
/// created (arbitrage's per-pool logs) — which one to pass is a caller
/// choice, not a guess, since guessing wrong would silently corrupt an
/// existing binary's log format.
pub fn append_trade_log_line(path: &str, line: &str, header: Option<&str>) {
    let needs_header = header.is_some() && !Path::new(path).exists();
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| -> std::io::Result<()> {
            if needs_header {
                writeln!(f, "{}", header.unwrap())?;
            }
            writeln!(f, "{line}")?;
            Ok(())
        });
    if let Err(e) = result {
        eprintln!("Warning: could not write to trade log ({path}): {e}");
    }
}

/// Every row — OPEN, CLOSE or MISFIRE — uses this exact column layout. Only
/// close_id/gain/roi/apr are ever blank (OPEN doesn't have them yet);
/// everything else, including prim_amount, is filled on both row types.
#[allow(clippy::too_many_arguments)]
pub fn log_row(
    path: &str,
    header: Option<&str>,
    kind: &str,
    pivot_id: Option<u32>,
    close_id: Option<u32>,
    opened_pivot_id: Option<u32>,
    prim: &str,
    proper: &str,
    prim_amount: f64,
    proper_amount: f64,
    gain: Option<f64>,
    roi: Option<f64>,
    apr: Option<f64>,
    gas_avax: f64,
    tx_hash: &str,
    snap: &BalanceSnapshot,
    cum: &CumulativeStats,
) {
    let pivot_id_s = pivot_id.map(|v| v.to_string()).unwrap_or_default();
    let close_id_s = close_id.map(|v| v.to_string()).unwrap_or_default();
    let opened_pivot_id_s = opened_pivot_id.map(|v| v.to_string()).unwrap_or_default();
    let gain_s = gain.map(|v| format!("{v:+.8}")).unwrap_or_default();
    let roi_s = roi.map(|v| format!("{v:.6}")).unwrap_or_default();
    let apr_s = apr.map(|v| format!("{v:.6}")).unwrap_or_default();
    append_trade_log_line(path, &format!(
        "{}\t{kind}\t{pivot_id_s}\t{close_id_s}\t{opened_pivot_id_s}\t{prim}\t{proper}\t{prim_amount:.8}\t{proper_amount:.8}\t{gain_s}\t{roi_s}\t{apr_s}\t{gas_avax:.8}\t{tx_hash}\t{}",
        log_ts(now_ts()), snapshot_and_cumulative_columns(snap, cum)
    ), header);
}

#[allow(clippy::too_many_arguments)]
pub fn log_open(path: &str, header: Option<&str>, pivot_id: u32, prim: &str, prim_amount: f64, proper: &str, proper_amount: f64, gas_avax: f64, tx_hash: &str, snap: &BalanceSnapshot, cum: &CumulativeStats) {
    log_row(path, header, "OPEN", Some(pivot_id), None, None, prim, proper, prim_amount, proper_amount, None, None, None, gas_avax, tx_hash, snap, cum);
}

#[allow(clippy::too_many_arguments)]
pub fn log_close(path: &str, header: Option<&str>, pivot_id: u32, close_id: u32, prim: &str, prim_amount: f64, proper: &str, proper_amount: f64, gain: f64, roi: f64, apr: f64, gas_avax: f64, tx_hash: &str, snap: &BalanceSnapshot, cum: &CumulativeStats) {
    log_row(path, header, "CLOSE", None, Some(close_id), Some(pivot_id), prim, proper, prim_amount, proper_amount, Some(gain), Some(roi), Some(apr), gas_avax, tx_hash, snap, cum);
}

/// Replays a pivot log from scratch — no in-memory state carried between
/// runs, the log file itself is the source of truth. A missing file
/// replays as empty (id counters starting at 1, zeroed stats): a fresh
/// pool with no history yet is a valid starting state as far as this
/// function is concerned. A binary that instead expects pre-existing
/// history (tvá, wired to one real pair) checks for the file itself
/// before calling this and hard-errors on its own terms if it's missing —
/// see e.g. tvá's `replay_log_with_history_required`.
pub fn replay_log(path: &str) -> ErrStr<(Vec<OpenPivot>, u32, u32, CumulativeStats)> {
    if !Path::new(path).exists() {
        return Ok((Vec::new(), 1, 1, CumulativeStats::default()));
    }

    let lines = lines_from_file(path)
        .map_err(|e| format!("Could not open trade log at '{path}': {e}"))?;

    let mut open_by_id: HashMap<u32, OpenPivot> = HashMap::new();
    let mut max_pivot_id: u32 = 0;
    let mut max_close_id: u32 = 0;
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
                let pivot_id: u32 = pivot_id_field.parse()
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
                let close_id: u32 = fields[3].parse()
                    .map_err(|_| format!("bad close_id at {path}:{}: '{line}'", line_no + 1))?;
                let opened_pivot_id: u32 = opened_pivot_id_field.parse()
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
pub enum AttemptOutcome {
    NotCleared,
    DryRunWouldClear { quoted_amount_out: f64 },
    Executed { tx_hash: String, actual_received: f64, gas_avax: f64 },
}

/// Quotes, checks the floor, and (unless dry-running) executes — the one
/// trade-attempt pipeline every pivot open/close in this system goes
/// through, regardless of which binary is calling it.
#[allow(clippy::too_many_arguments)]
pub async fn attempt_trade_with_actual_amount(
    wallet_address: &str,
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
    min_floor: f64,
    slippage_bps: u16,
    keystore_path_var: &str,
    dry_run: bool,
    debug: bool,
) -> ErrStr<AttemptOutcome> {
    let swap = kyber_swap(registry, from_symbol, to_symbol, amount).await?;
    if swap.amount_out <= min_floor {
            debug_trade_result(None, "NOT CLEARED", from_symbol, to_symbol, amount, &swap, min_floor, debug);
    
        Ok(AttemptOutcome::NotCleared)
    } else {
        if dry_run {
                debug_trade_result(None, "DRY-RUN WOULD CLEAR", from_symbol, to_symbol, amount, &swap, min_floor, debug);
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out: swap.amount_out })
        } else {
            let balance_before = wallet_balance(wallet_address, to_symbol, registry).await?;
            let (tx_hash, gas_avax) = execute_trade(wallet_address, registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, keystore_path_var, debug).await?;
            let balance_after = wallet_balance(wallet_address, to_symbol, registry).await?;
            let actual_received = balance_after - balance_before;
                debug_trade_result(Some(&tx_hash), "EXECUTED", from_symbol, to_symbol, amount, &swap, min_floor, debug);
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax })
        }
    }
}

fn debug_trade_result(tx: Option<&str>, kind: &str, from_symbol: &str, to_symbol: &str, amount: f64, swap: &KyberSwap, min_floor: f64, debug: bool) {
    if debug {
        eprintln!(
            "[{kind}] Trade {from_symbol} -> {to_symbol} {} swap {amount:.4} -> {amount_out:.4} (floor {floor:.4})",
            if tx.is_some() { format!("tx {}", tx.unwrap()) } else { s("") },
            amount_out = swap.amount_out,
            floor = min_floor
        );
    }
}

//============================================================================
//----- Signing & Execution ----------------------------------------------------
//============================================================================
// Everything past this point can move real funds. Every function here is
// deliberately loud on failure.

fn pad_u256_for_call(amount: u128) -> String {
    let ans = format!("{amount:064x}");
    ans
}

/// AVAX cost of a confirmed transaction, computed from its own receipt
/// (gas_used * effective_gas_price), not estimated beforehand. If a
/// receipt is somehow missing pricing info, returns 0.0 rather than
/// failing the whole trade over a cosmetic figure — the trade itself
/// already succeeded by the time this is called.
fn gas_cost_avax(gas_used: Option<U256>, effective_gas_price: Option<U256>) -> f64 {
    match (gas_used, effective_gas_price) {
        (Some(g), Some(p)) => {
            let wei = g.saturating_mul(p);
            wei.as_u128() as f64 / 1e18
        }
        _ => 0.0,
    }
}

pub async fn load_signer(expected_address: &str, keystore_path_var: &str) -> ErrStr<LocalWallet> {
    let keystore_path = std::env::var(keystore_path_var).map_err(|_| {
        format!("Missing required env var: {keystore_path_var} (full path to the encrypted keystore file). No funds moved.")
    })?;
    let password = match std::env::var("KEYSTORE_PASSWORD") {
        Ok(pw) => pw,
        Err(_) => rpassword::prompt_password("Keystore password: ")
            .map_err(|e| format!("Could not read password: {e}. No funds moved."))?,
    };
    let wallet = LocalWallet::decrypt_keystore(&keystore_path, &password)
        .map_err(|e| format!("Could not decrypt keystore: {e}. No funds moved."))?
        .with_chain_id(AVALANCHE_CHAIN_ID);
    let derived = format!("{:?}", wallet.address());
    if !derived.eq_ignore_ascii_case(expected_address) {
        return Err(format!(
            "Keystore address ({derived}) does not match expected address ({expected_address}) — refusing to proceed. No funds moved."
        ));
    }
    Ok(wallet)
}

/// Builds an EIP-1559 tx with a buffered max fee (so a base-fee bump between
/// estimation and submission — e.g. during the password prompt — doesn't
/// get the tx rejected pre-mempool) and a buffered gas limit. Fees are
/// re-estimated fresh on every call rather than reused across steps.
async fn build_tx_with_fee_buffer(
    client: &SignerMiddleware<Provider<Http>, LocalWallet>,
    to: Address,
    data: Bytes,
) -> ErrStr<Eip1559TransactionRequest> {
    let (max_fee, max_priority_fee) = client
        .estimate_eip1559_fees(None)
        .await
        .map_err(|e| format!("Could not estimate EIP-1559 fees: {e}"))?;

    // 30% buffer on the max fee absorbs a base-fee bump between estimation
    // and submission without overpaying on the priority fee.
    let buffered_max_fee = max_fee.saturating_mul(U256::from(130)) / U256::from(100);

    let mut tx = Eip1559TransactionRequest::new()
        .to(to)
        .data(data)
        .max_fee_per_gas(buffered_max_fee)
        .max_priority_fee_per_gas(max_priority_fee);

    let typed: TypedTransaction = tx.clone().into();
    let gas_estimate = client
        .estimate_gas(&typed, None)
        .await
        .map_err(|e| format!("Could not estimate gas limit: {e}"))?;
    // 20% buffer on gas so a slightly-off estimate doesn't run out mid-execution.
    let buffered_gas = gas_estimate.saturating_mul(U256::from(120)) / U256::from(100);
    tx = tx.gas(buffered_gas);

    Ok(tx)
}

/// Approves the router for EXACTLY this trade's amount — never a standing
/// allowance. The router can never pull more than what's approved here.
pub async fn approve_exact_amount(
    client: &SignerMiddleware<Provider<Http>, LocalWallet>,
    token_contract: &str,
    spender: &str,
    amount_base_units: u128,
    verbose: bool,
) -> ErrStr<f64> {
    let data_hex = format!(
        "0x095ea7b3{}{}",
        pad_address_for_call(spender),
        pad_u256_for_call(amount_base_units)
    );
    let to = Address::from_str(token_contract).map_err(|e| format!("Bad token address: {e}"))?;
    let data = Bytes::from_str(&data_hex).map_err(|e| format!("Bad approve calldata: {e}"))?;
    let tx = build_tx_with_fee_buffer(client, to, data).await?;

    let pending = client
        .send_transaction(tx, None)
        .await
        .map_err(|e| format!("Approve transaction failed to send: {e}"))?;
    if verbose {
        println!("    Approve tx submitted: {:?}", pending.tx_hash());
    }

    let receipt = pending
        .await
        .map_err(|e| format!("Approve transaction failed while confirming: {e}"))?;
    match receipt {
        Some(r) => {
            if verbose {
                println!("    Approve confirmed in block {:?}", r.block_number);
            }
            Ok(gas_cost_avax(r.gas_used, r.effective_gas_price))
        }
        None => Err("Approve transaction was dropped or replaced".to_string()),
    }
}

/// Asks KyberSwap to encode the actual swap calldata for the route.
/// When `verbose`, prints the raw response so it can be eyeballed before
/// trusting it — otherwise that's a screen-filling blob of hex calldata,
/// so it stays silent by default. `slippage_bps` is basis points (e.g.
/// 50 = 0.50%).
pub async fn kyberswap_build(
    route_summary_raw: &serde_json::Value,
    sender: &str,
    slippage_bps: u16,
    verbose: bool,
) -> ErrStr<(String, String)> {
    let body = serde_json::json!({
        "routeSummary": route_summary_raw,
        "sender": sender,
        "recipient": sender,
        "slippageTolerance": slippage_bps
    });

    let resp = http_client()?
        .post(format!("https://aggregator-api.kyberswap.com/{KYBERSWAP_CHAIN}/api/v1/route/build"))
        .header("X-Client-Id", "pivoteur-autotrader")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("KyberSwap build request failed: {e}"))?;

    let status = resp.status();
    let raw_body = resp.text().await.map_err(|e| format!("Could not read build response: {e}"))?;
    if verbose {
        println!("    KyberSwap build response (verify this looks right):\n    {raw_body}");
    }

    let parsed: serde_json::Value = serde_json::from_str(&raw_body).map_err(|e| {
        format!("KyberSwap build response did not parse (HTTP {status}): {e}\nRaw body: {raw_body}")
    })?;
    let data = parsed
        .get("data")
        .ok_or_else(|| format!("Build response has no data. Raw: {raw_body}"))?;
    let router = data
        .get("routerAddress")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Build response missing routerAddress. Raw: {raw_body}"))?
        .to_string();
    let calldata = data
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Build response missing calldata. Raw: {raw_body}"))?
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
    verbose: bool,
) -> ErrStr<(String, f64)> {
    let to = Address::from_str(router).map_err(|e| format!("Bad router address: {e}"))?;
    let data = Bytes::from_str(calldata_hex).map_err(|e| format!("Bad calldata from KyberSwap: {e}"))?;
    let tx = build_tx_with_fee_buffer(client, to, data).await?;

    let pending = client
        .send_transaction(tx, None)
        .await
        .map_err(|e| format!("Swap transaction failed to send: {e}"))?;
    let tx_hash = format!("{:?}", pending.tx_hash());
    if verbose {
        println!("    Swap tx submitted: {tx_hash}");
    }

    let receipt = pending
        .await
        .map_err(|e| format!("Swap transaction failed while confirming: {e}"))?;
    match receipt {
        Some(r) if r.status == Some(1.into()) => {
            if verbose {
                println!("    Swap confirmed in block {:?}", r.block_number);
            }
            Ok((tx_hash, gas_cost_avax(r.gas_used, r.effective_gas_price)))
        }
        Some(_) => Err(format!("Swap transaction REVERTED on-chain. Hash: {tx_hash}")),
        None => Err(format!("Swap transaction was dropped or replaced. Hash: {tx_hash}")),
    }
}

/// Sends `amount` of `symbol` from the wallet straight to `to_address` —
/// a plain ERC-20 transfer(), not a swap. Same safety baseline as
/// execute_trade: verified signer, EIP-1559 fee buffering, hard error on
/// revert/drop/failure rather than a false success. Does not support
/// native AVAX (no tokens.toml address to encode against) — only ERC-20s
/// with a real contract address.
pub async fn send_tokens(
    wallet_address: &str,
    registry: &TokenRegistry,
    symbol: &str,
    to_address_var: &str,
    amount: f64,
    keystore_path_var: &str,
    verbose: bool,
) -> ErrStr<(String, f64)> {
    if amount <= 0.0 {
        return Err(format!("send_tokens: amount must be positive, got {amount}"));
    }

    let signer = load_signer(wallet_address, keystore_path_var).await?;
    let provider = Provider::<Http>::try_from(AVALANCHE_RPC)
        .map_err(|e| format!("Could not create RPC provider: {e}"))?;
    let client = SignerMiddleware::new(provider, signer);

    let entry = token_entry(registry, symbol)?;
    let token_addr = entry.address.as_deref().ok_or_else(|| {
        format!("'{symbol}' has no address in tokens.toml (or is native — send_tokens only supports ERC-20 transfers)")
    })?;
    let amount_base = (amount * 10f64.powi(entry.decimals as i32)).round() as u128;

    let to_address = get_env(to_address_var)?;
    // transfer(address,uint256) selector = 0xa9059cbb
    let data_hex = format!(
        "0xa9059cbb{}{}",
        pad_address_for_call(&to_address),
        pad_u256_for_call(amount_base)
    );
    let to = Address::from_str(token_addr).map_err(|e| format!("Bad token address: {e}"))?;
    let data = Bytes::from_str(&data_hex).map_err(|e| format!("Bad transfer calldata: {e}"))?;
    let tx = build_tx_with_fee_buffer(&client, to, data).await?;

    if !verbose {
        println!("    On its way to the vault — courier's en route...");
    }

    let pending = client
        .send_transaction(tx, None)
        .await
        .map_err(|e| format!("Transfer transaction failed to send: {e}"))?;
    let tx_hash = format!("{:?}", pending.tx_hash());
    if verbose {
        println!("    Transfer tx submitted: {tx_hash}");
    }

    let receipt = pending
        .await
        .map_err(|e| format!("Transfer transaction failed while confirming: {e}"))?;
    match receipt {
        Some(r) if r.status == Some(1.into()) => {
            if verbose {
                println!("    Transfer confirmed in block {:?}", r.block_number);
            }
            Ok((tx_hash, gas_cost_avax(r.gas_used, r.effective_gas_price)))
        }
        Some(_) => Err(format!("Transfer transaction REVERTED on-chain. Hash: {tx_hash}")),
        None => Err(format!("Transfer transaction was dropped or replaced. Hash: {tx_hash}")),
    }
}

//============================================================================
//----- UNIT TESTS -------------------------------------------------------------
//============================================================================
#[cfg(test)]
mod unit_tests {
    use super::*;


    #[test]
    fn test_hex_to_u128_parses_rpc_style_hex() -> ErrStr<()> {
        assert_eq!(hex_to_u128("0x0")?, 0);
        assert_eq!(hex_to_u128("0x")?, 0);
        assert_eq!(hex_to_u128("0xff")?, 255);
        assert_eq!(hex_to_u128("0xde0b6b3a7640000")?, 1_000_000_000_000_000_000);
        Ok(())
    }

    #[test]
    fn test_hex_to_u128_rejects_garbage() {
        assert!(hex_to_u128("0xnotarealnumber").is_err());
    }

    #[test]
    fn test_pad_address_for_call_produces_32_byte_word() {
        let padded = pad_address_for_call("0x69b21DC480CA62E478D997d7313061F765a5B122");
        assert_eq!(padded.len(), 64);
        assert!(padded.ends_with("69b21dc480ca62e478d997d7313061f765a5b122"));
        assert!(padded.starts_with("00000000000000000000"));
    }

    #[test]
    fn test_log_ts_round_trips_through_utc_without_drift() {
        for epoch in [0u64, 1_000, 1_785_896_548, 1_785_933_872] {
            let formatted = log_ts(epoch);
            let parsed = parse_log_ts(&formatted).expect("a freshly-formatted timestamp must parse back cleanly");
            assert_eq!(parsed, epoch, "round-tripping epoch {epoch} through '{formatted}' should recover the exact same second, not just the same day");
        }
    }

    #[test]
    fn test_biggest_first_sorts_by_raw_proper_amount_descending() {
        let make = |id: u32, proper_amount: f64| OpenPivot {
            pivot_id: id, opened_at: 0, prim: "X".into(), prim_amount: 0.0,
            proper: "Y".into(), proper_amount,
        };
        let pivots = vec![make(1, 500_000.0), make(2, 0.005), make(3, 520_000.0)];
        let sorted = biggest_first(pivots);
        let ids: Vec<u32> = sorted.iter().map(|p| p.pivot_id).collect();
        assert_eq!(ids, vec![3, 1, 2], "should be ordered biggest proper_amount to smallest, raw number, no currency conversion");
    }

    #[test]
    fn test_replay_log_missing_file_replays_as_a_fresh_empty_pool() -> ErrStr<()> {
        let (opens, next_pivot, next_close, stats) = replay_log("/tmp/definitely_does_not_exist.log")?;
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
}

pub async fn execute_trade(
    wallet_address: &str,
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
    min_floor: f64,
    slippage_bps: u16,
    keystore_path_var: &str,
    verbose: bool,
) -> ErrStr<(String, f64)> {
    let signer = load_signer(wallet_address, keystore_path_var).await?;
    let provider = Provider::<Http>::try_from(AVALANCHE_RPC)
        .map_err(|e| format!("Could not create RPC provider: {e}"))?;
    let client = SignerMiddleware::new(provider, signer);

    if !verbose {
        println!("  Trading in progress — approve, quote, swap. This part takes a minute...");
    }
    if verbose {
        println!(">>> Re-checking the quote after keystore unlock (it may have moved)...");
    }
    let fresh_quote = kyber_swap(registry, from_symbol, to_symbol, amount).await?;
    if verbose {
        println!("Fresh quote: {amount:.6} {from_symbol} -> {:.8} {to_symbol} now", fresh_quote.amount_out);
    }
    if fresh_quote.amount_out < min_floor {
        return Err(format!(
            "Quote moved below your floor while unlocking the keystore ({:.8} {to_symbol} < {min_floor:.8} {to_symbol}). \
             That's not happening. No funds used.",
            fresh_quote.amount_out
        ));
    }

    let from_entry = token_entry(registry, from_symbol)?;
    let from_addr = from_entry.address.as_deref().ok_or_else(|| format!("{from_symbol} missing address"))?.to_string();
    let amount_base = (amount * 10f64.powi(from_entry.decimals as i32)).round() as u128;

    if verbose {
        println!(">>> Approving exact amount ({amount:.6} {from_symbol}) for the router...");
    }
    let approve_gas = approve_exact_amount(&client, &from_addr, &fresh_quote.router_address, amount_base, verbose).await?;

    if verbose {
        println!(">>> Requesting swap calldata from KyberSwap...");
    }
    let (router, calldata) =
        kyberswap_build(&fresh_quote.route_summary_raw, wallet_address, slippage_bps, verbose).await?;

    if verbose {
        println!(">>> Sending swap transaction...");
    }
    let (tx_hash, swap_gas) = send_swap_tx(&client, &router, &calldata, verbose).await?;

    Ok((tx_hash, approve_gas + swap_gas))
}
