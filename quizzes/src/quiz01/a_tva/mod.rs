use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        parse_args_add_banner,
};
use libs::auto_trading::{
            TokenRegistry, 
            parse_token_registry, 
            wallet_address_from_env,
            wallet_balance, 
            live_quote, 
            execute_trade,
            send_tokens,
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================
const TOKENS_TOML: &str = include_str!("tokens.toml");

pub fn load_token_registry() -> ErrStr<TokenRegistry> {
    parse_token_registry(TOKENS_TOML)
}

//============================================================================
//----- Fixed Trade Sizes ------------------------------------------------------
//============================================================================
const UNDEAD_TRADE_AMOUNT: f64 = 500_000.0;
const BTC_TRADE_AMOUNT: f64 = 0.005;
const SLIPPAGE_BPS: u16 = 200;

// The capital Doug entrusted tvá with at the start. Used purely for the
// "vs starting capital" line below — never used as a floor or trigger for
// anything that moves funds. That's a separate, not-yet-built feature.
const STARTING_UNDEAD: f64 = 5_000_000.0;
const STARTING_BTC: f64 = 0.05;

// Illustrative only — NOT wired to any real transfer, and no funds move
// here. Just a preview of what an even split of the surplus above starting
// capital would look like, so there's real data to look at.
const ILLUSTRATIVE_SKIM_PCT: f64 = 0.50;
const VAULT_ADDRESS: &str = "0xe25835EF625ecE064d899844e17Ea59742369A92";

// `div` subcommand: unlike the preview above, this ACTUALLY sends funds.
// Default percentage of the sendable surplus that goes to Vault; the rest
// 'trim' simply stays in tvá's own wallet.
const DEFAULT_DIV_PCT: f64 = 25.0;

// This tiny epsilon exists purely
// as self-documentation: it makes it obvious at a glance that opens
// aren't checking for a real economic floor the way closes do.
const NO_REAL_FLOOR: f64 = 0.000_000_01;

//============================================================================
//----- Trade Log — format & replay --------------------------------------------
//============================================================================
const TRADE_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/tva-trades.log");

#[derive(Debug, Clone)]
struct OpenPivot {
    pivot_id:      u32,
    opened_at:     u64,
    prim:          String,
    prim_amount:   f64,
    proper:        String,
    proper_amount: f64,
}

/// Cumulative totals across every CLOSE (and gas across every OPEN+CLOSE)
/// in the log's history. Gains are kept in separate BTC/UNDEAD buckets
/// since a pivot's gain is denominated in whichever token is its "prim" —
/// summing across the two currencies directly would be meaningless.
#[derive(Debug, Clone, Default)]
struct CumulativeStats {
    total_opens:      u32,
    total_closes:     u32,
    total_gain_btc:    f64,
    total_gain_undead: f64,
    total_gas_avax:    f64,
    roi_sum: f64,
    apr_sum: f64,
}

impl CumulativeStats {
    fn avg_roi(&self) -> f64 {
        if self.total_closes == 0 { 0.0 } else { self.roi_sum / self.total_closes as f64 }
    }
    fn avg_apr(&self) -> f64 {
        if self.total_closes == 0 { 0.0 } else { self.apr_sum / self.total_closes as f64 }
    }
}

/// Wallet state at a single point in time: on-chain balance, how much of
/// that balance is committed to still-open pivots, and what's actually
/// free to trade with. `committed` is passed in rather than recomputed
/// here — the caller tracks it incrementally through the cycle (cheap,
/// in-memory) rather than this function reconstructing it from a pivot
/// list on every call.
#[derive(Debug, Clone, Copy)]
pub struct WalletSnapshot {
    btc_balance:      f64,
    btc_committed:    f64,
    btc_available:    f64,
    undead_balance:    f64,
    undead_committed:  f64,
    undead_available:  f64,
}

async fn wallet_snapshot(
    wallet_address: &str,
    registry: &TokenRegistry,
    btc_committed: f64,
    undead_committed: f64,
) -> ErrStr<WalletSnapshot> {
    let btc_balance = wallet_balance(wallet_address, "BTC", registry).await?;
    let undead_balance = wallet_balance(wallet_address, "UNDEAD", registry).await?;
    Ok(WalletSnapshot {
        btc_balance,
        btc_committed,
        btc_available: btc_balance - btc_committed,
        undead_balance,
        undead_committed,
        undead_available: undead_balance - undead_committed,
    })
}

fn now_ts() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn human_ts(epoch: u64) -> String {
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

const LOG_TS_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn log_ts(epoch: u64) -> String {
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.format(LOG_TS_FORMAT).to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

fn parse_log_ts(s: &str) -> ErrStr<u64> {
    chrono::NaiveDateTime::parse_from_str(s, LOG_TS_FORMAT)
        .map(|ndt| ndt.and_utc().timestamp() as u64)
        .map_err(|e| format!("bad timestamp '{s}' (expected UTC '{LOG_TS_FORMAT}', e.g. '2026-08-05 14:32:07'): {e}"))
}

fn append_log(line: &str) {
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRADE_LOG_PATH)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = result {
        eprintln!("Warning: could not write to trade log ({TRADE_LOG_PATH}): {e}");
    }
}

fn snapshot_and_cumulative_columns(snap: &WalletSnapshot, cum: &CumulativeStats) -> String {
    format!(
        "{:.8}\t{:.8}\t{:.8}\t{:.2}\t{:.2}\t{:.2}\t{:+.8}\t{:+.2}\t{:.8}\t{:.6}\t{:.6}",
        snap.btc_balance, snap.btc_committed, snap.btc_available,
        snap.undead_balance, snap.undead_committed, snap.undead_available,
        cum.total_gain_btc, cum.total_gain_undead, cum.total_gas_avax,
        cum.avg_roi(), cum.avg_apr(),
    )
}

fn log_open(pivot_id: u32, prim: &str, prim_amount: f64, proper: &str, proper_amount: f64, gas_avax: f64, tx_hash: &str, snap: &WalletSnapshot, cum: &CumulativeStats) {
    append_log(&format!(
        "{}\tOPEN\t{pivot_id}\t{prim}\t{prim_amount:.8}\t{proper}\t{proper_amount:.8}\t{gas_avax:.8}\t{tx_hash}\t{}",
        log_ts(now_ts()), snapshot_and_cumulative_columns(snap, cum)
    ));
}

fn log_close(pivot_id: u32, close_id: u32, proper_amount: f64, gain: f64, roi: f64, apr: f64, gas_avax: f64, tx_hash: &str, snap: &WalletSnapshot, cum: &CumulativeStats) {
    append_log(&format!(
        "{}\tCLOSE\t{pivot_id}\t{close_id}\t{proper_amount:.8}\t{gain:.8}\t{roi:.6}\t{apr:.6}\t{gas_avax:.8}\t{tx_hash}\t{}",
        log_ts(now_ts()), snapshot_and_cumulative_columns(snap, cum)
    ));
}

/// tvá's history predates this program and was wired in deliberately so it hits the ground running.
/// A missing file means the path is wrong (wrong repo, wrong working directory), and silently starting
/// from empty would quietly re-open pivots that already exist on-chain.
/// Hard error, always.
///
/// Single pass: builds the still-open pivot list AND the cumulative stats
/// together, so there's exactly one place that understands the log's
/// on-disk shape.
fn replay_log(path: &str) -> ErrStr<(Vec<OpenPivot>, u32, u32, CumulativeStats)> {
    let file = File::open(path).map_err(|e| format!(
        "Where is tvá's history? Could not open trade log at '{path}': {e}. \
         This program is wired to real, pre-existing trade history — a missing \
         log file means the path is wrong, not that this is a fresh start. Refusing to run."
    ))?;
    let reader = BufReader::new(file);

    let mut open_by_id: HashMap<u32, OpenPivot> = HashMap::new();
    let mut max_pivot_id: u32 = 0;
    let mut max_close_id: u32 = 0;
    let mut stats = CumulativeStats::default();

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("Could not read {path} at line {}: {e}", line_no + 1))?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.get(1) {
            Some(&"OPEN") => {
                if fields.len() < 9 {
                    return Err(format!("malformed OPEN line at {path}:{}: '{line}'", line_no + 1));
                }
                let opened_at: u64 = parse_log_ts(fields[0])
                    .map_err(|e| format!("{e} at {path}:{}: '{line}'", line_no + 1))?;
                let pivot_id: u32 = fields[2].parse()
                    .map_err(|_| format!("bad pivot_id at {path}:{}: '{line}'", line_no + 1))?;
                let prim_amount: f64 = fields[4].parse()
                    .map_err(|_| format!("bad prim_amount at {path}:{}: '{line}'", line_no + 1))?;
                let proper_amount: f64 = fields[6].parse()
                    .map_err(|_| format!("bad proper_amount at {path}:{}: '{line}'", line_no + 1))?;
                let gas_avax: f64 = fields[7].parse()
                    .map_err(|_| format!("bad gas_avax at {path}:{}: '{line}'", line_no + 1))?;
                max_pivot_id = max_pivot_id.max(pivot_id);
                stats.total_opens += 1;
                stats.total_gas_avax += gas_avax;
                open_by_id.insert(pivot_id, OpenPivot {
                    pivot_id,
                    opened_at,
                    prim: fields[3].to_string(),
                    prim_amount,
                    proper: fields[5].to_string(),
                    proper_amount,
                });
            }
            Some(&"CLOSE") => {
                if fields.len() < 10 {
                    return Err(format!("malformed CLOSE line at {path}:{}: '{line}'", line_no + 1));
                }
                let pivot_id: u32 = fields[2].parse()
                    .map_err(|_| format!("bad pivot_id at {path}:{}: '{line}'", line_no + 1))?;
                let close_id: u32 = fields[3].parse()
                    .map_err(|_| format!("bad close_id at {path}:{}: '{line}'", line_no + 1))?;
                let gain: f64 = fields[5].parse()
                    .map_err(|_| format!("bad gain at {path}:{}: '{line}'", line_no + 1))?;
                let roi: f64 = fields[6].parse()
                    .map_err(|_| format!("bad roi at {path}:{}: '{line}'", line_no + 1))?;
                let apr: f64 = fields[7].parse()
                    .map_err(|_| format!("bad apr at {path}:{}: '{line}'", line_no + 1))?;
                let gas_avax: f64 = fields[8].parse()
                    .map_err(|_| format!("bad gas_avax at {path}:{}: '{line}'", line_no + 1))?;

                let closed_pivot = open_by_id.remove(&pivot_id)
                    .ok_or_else(|| format!(
                        "CLOSE at {path}:{} references pivot #{pivot_id}, which has no matching OPEN before it",
                        line_no + 1
                    ))?;

                max_close_id = max_close_id.max(close_id);
                stats.total_closes += 1;
                stats.total_gas_avax += gas_avax;
                stats.roi_sum += roi;
                stats.apr_sum += apr;
                match closed_pivot.prim.as_str() {
                    "BTC" => stats.total_gain_btc += gain,
                    "UNDEAD" => stats.total_gain_undead += gain,
                    other => eprintln!("Warning: CLOSE for pivot #{pivot_id} has unrecognized prim '{other}' — gain not added to either cumulative bucket"),
                }
            }
            Some(&"CHECK") => {}
            _ => return Err(format!("unrecognized log line at {path}:{}: '{line}'", line_no + 1)),
        }
    }

    let still_open: Vec<OpenPivot> = open_by_id.into_values().collect();
    Ok((still_open, max_pivot_id + 1, max_close_id + 1, stats))
}

/// Largest trade first: sort candidates for closing by their raw
/// proper_amount, descending. Pulled out as its own pure function so it's
/// testable without touching the network.
fn biggest_first(mut pivots: Vec<OpenPivot>) -> Vec<OpenPivot> {
    pivots.sort_by(|a, b| {
        b.proper_amount.partial_cmp(&a.proper_amount).unwrap_or(std::cmp::Ordering::Equal)
    });
    pivots
}

//============================================================================
//----- One Trading Cycle -------------------------------------------------------
//============================================================================
enum AttemptOutcome {
    NotCleared,
    DryRunWouldClear { quoted_amount_out: f64 },
    Executed { tx_hash: String, actual_received: f64, gas_avax: f64 },
}

async fn attempt_trade_with_actual_amount(
    wallet_address: &str,
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
    min_floor: f64,
    dry_run: bool,
) -> ErrStr<AttemptOutcome> {
    let quote = live_quote(registry, from_symbol, to_symbol, amount).await?;
    if quote.amount_out <= min_floor {
        return Ok(AttemptOutcome::NotCleared);
    }

    if dry_run {
        return Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out: quote.amount_out });
    }

    let balance_before = wallet_balance(wallet_address, to_symbol, registry).await?;
    let (tx_hash, gas_avax) = execute_trade(wallet_address, registry, from_symbol, to_symbol, amount, min_floor, SLIPPAGE_BPS, "TVA_KEYSTORE_PATH").await?;
    let balance_after = wallet_balance(wallet_address, to_symbol, registry).await?;
    let actual_received = balance_after - balance_before;

    Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax })
}

pub async fn run_cycle(dry_run: bool) -> ErrStr<WalletSnapshot> {
    let wallet_address = wallet_address_from_env("TVA_WALLET_ADDRESS")?;
    if let Ok(expected) = std::env::var("TVA_EXPECTED_WALLET") {
        if !expected.eq_ignore_ascii_case(&wallet_address) {
            return Err(format!(
                "WALLET_ADDRESS ({wallet_address}) does not match TVA_EXPECTED_WALLET ({expected}) — refusing to run."
            ));
        }
    }

    let registry = load_token_registry()?;
    let (open_pivots, mut next_pivot_id, mut next_close_id, opening_stats) = replay_log(TRADE_LOG_PATH)?;
    let open_pivots = biggest_first(open_pivots);

    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };
    println!("tvá — {}{mode_tag} — {} open pivot(s), wallet {wallet_address}", human_ts(now_ts()), open_pivots.len());

    // Committed amounts, tracked incrementally through the cycle rather than
    // recomputed from a pivot list each time: start at the full committed
    // total across everything open right now, decrement when something
    // closes, increment when something new opens. Pivots that stay open
    // (NotCleared / Err) need no adjustment — they were already counted in
    // the starting sum.
    let mut committed_btc: f64 = open_pivots.iter().filter(|p| p.proper == "BTC").map(|p| p.proper_amount).sum();
    let mut committed_undead: f64 = open_pivots.iter().filter(|p| p.proper == "UNDEAD").map(|p| p.proper_amount).sum();
    let mut running_stats = opening_stats;

    let mut closed_something = false;

    for pivot in open_pivots {
        match attempt_trade_with_actual_amount(
            &wallet_address, &registry, &pivot.proper, &pivot.prim,
            pivot.proper_amount, pivot.prim_amount, dry_run,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                let gain = actual_received - pivot.prim_amount;
                let roi = gain / pivot.prim_amount;
                let days_held = (now_ts().saturating_sub(pivot.opened_at)) as f64 / 86_400.0;
                let apr = if days_held > 0.0 { roi * 365.0 / days_held } else { 0.0 };
                println!(
                    "  CLOSED  #{:<4} {:.4} {} -> {:.4} {}   gain {:+.4} {}   roi {:.2}%   apr {:.1}%   gas {:.5} AVAX",
                    pivot.pivot_id, pivot.proper_amount, pivot.proper, actual_received, pivot.prim,
                    gain, pivot.prim, roi * 100.0, apr * 100.0, gas_avax
                );

                match pivot.proper.as_str() {
                    "BTC" => committed_btc -= pivot.proper_amount,
                    "UNDEAD" => committed_undead -= pivot.proper_amount,
                    _ => {}
                }
                running_stats.total_closes += 1;
                running_stats.total_gas_avax += gas_avax;
                running_stats.roi_sum += roi;
                running_stats.apr_sum += apr;
                match pivot.prim.as_str() {
                    "BTC" => running_stats.total_gain_btc += gain,
                    "UNDEAD" => running_stats.total_gain_undead += gain,
                    _ => {}
                }

                let snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
                log_close(pivot.pivot_id, next_close_id, actual_received, gain, roi, apr, gas_avax, &tx_hash, &snap, &running_stats);
                next_close_id += 1;
                closed_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                let gain = quoted_amount_out - pivot.prim_amount;
                let roi = gain / pivot.prim_amount;
                println!(
                    "  WOULD CLOSE  #{:<4} {:.4} {} -> ~{:.4} {}   est. gain {:+.4} {}   est. roi {:.2}%   (dry run, no funds moved)",
                    pivot.pivot_id, pivot.proper_amount, pivot.proper, quoted_amount_out, pivot.prim,
                    gain, pivot.prim, roi * 100.0
                );
                closed_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => {}
            Err(e) => {
                println!("  ! close attempt for pivot #{} failed, staying open: {e}", pivot.pivot_id);
            }
        }
    }

    if !closed_something {
        println!("  Nothing to close this cycle — see ya in an hour!");
    }

    let mut snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
    println!(
        "Wallet's Status = BTC {:.6} in wallet ({:.6} committed, {:.6} available) | UNDEAD {:.2} in wallet ({:.2} committed, {:.2} available)",
        snap.btc_balance, snap.btc_committed, snap.btc_available, snap.undead_balance, snap.undead_committed, snap.undead_available
    );

    let mut opened_something = false;
    let mut skipped_open_reasons: Vec<String> = Vec::new();

    if snap.btc_available > BTC_TRADE_AMOUNT {
        let attempted_id = next_pivot_id;
        match attempt_trade_with_actual_amount(
            &wallet_address, &registry, "BTC", "UNDEAD", BTC_TRADE_AMOUNT, NO_REAL_FLOOR, dry_run,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.4} BTC -> {actual_received:.2} UNDEAD   gas {gas_avax:.5} AVAX");
                committed_undead += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
                log_open(attempted_id, "BTC", BTC_TRADE_AMOUNT, "UNDEAD", actual_received, gas_avax, &tx_hash, &snap, &running_stats);
                opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.4} BTC -> ~{quoted_amount_out:.2} UNDEAD   (dry run, no funds moved)");
                opened_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => println!("  ! unexpected: BTC->UNDEAD open quote didn't clear the near-zero floor — check the pool."),
            Err(e) => println!("  ! open (BTC->UNDEAD) failed: {e}"),
        }
        next_pivot_id += 1;
    } else {
        skipped_open_reasons.push(format!("BTC free balance {:.6} <= {BTC_TRADE_AMOUNT}", snap.btc_available));
    }

    if snap.undead_available > UNDEAD_TRADE_AMOUNT {
        match attempt_trade_with_actual_amount(
            &wallet_address, &registry, "UNDEAD", "BTC", UNDEAD_TRADE_AMOUNT, NO_REAL_FLOOR, dry_run,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> {actual_received:.6} BTC   gas {gas_avax:.5} AVAX");
                committed_btc += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
                log_open(next_pivot_id, "UNDEAD", UNDEAD_TRADE_AMOUNT, "BTC", actual_received, gas_avax, &tx_hash, &snap, &running_stats);
                opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> ~{quoted_amount_out:.6} BTC   (dry run, no funds moved)");
                opened_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => println!("  ! unexpected: UNDEAD->BTC open quote didn't clear the near-zero floor — check the pool."),
            Err(e) => println!("  ! open (UNDEAD->BTC) failed: {e}"),
        }
    } else {
        skipped_open_reasons.push(format!("UNDEAD free balance {:.2} <= {UNDEAD_TRADE_AMOUNT}", snap.undead_available));
    }

    if !opened_something {
        println!("  Nothing to open this cycle ({}) — see ya in an hour!", skipped_open_reasons.join("; "));
    }

    println!(
        "  Totals   {} closes, {} opens   gain BTC {:+.6}   gain UNDEAD {:+.2}   gas {:.5} AVAX   avg roi {:.2}%   avg apr {:.1}%",
        running_stats.total_closes, running_stats.total_opens,
        running_stats.total_gain_btc, running_stats.total_gain_undead, running_stats.total_gas_avax,
        running_stats.avg_roi() * 100.0, running_stats.avg_apr() * 100.0
    );

    // `snap` here is the freshest one taken this cycle — refreshed after
    // each real open, so it's post-action state. In 'dry-run mode' nothing
    // ever actually executes (attempt_trade_with_actual_amount returns
    // before touching the keystore), so this is still a real, live
    // balance query — not a simulated number — meaning both lines below
    // are meaningful in --dry-run too, not just real runs.
    let btc_vs_start = snap.btc_balance - STARTING_BTC;
    let undead_vs_start = snap.undead_balance - STARTING_UNDEAD;
    println!(
        "  Vs starting capital ({STARTING_UNDEAD} UNDEAD / {STARTING_BTC} BTC entrusted): BTC {btc_vs_start:+.6}   UNDEAD {undead_vs_start:+.2}"
    );

    // Illustrative split PREVIEW only — no funds move here, nothing is
    // sent anywhere. Just shows what an even split of today's surplus
    // would look like against VAULT_ADDRESS, so there's real numbers
    // to look at before/ if a skim feature gets built.
    let kept_pct = (1.0 - ILLUSTRATIVE_SKIM_PCT) * 100.0;
    let sent_pct = ILLUSTRATIVE_SKIM_PCT * 100.0;
    if btc_vs_start > 0.0 {
        let btc_kept = btc_vs_start * (1.0 - ILLUSTRATIVE_SKIM_PCT);
        let btc_sent = btc_vs_start * ILLUSTRATIVE_SKIM_PCT;
        println!(
            "  Split preview (illustrative, no funds moved) — BTC kept {btc_kept:+.6} ({kept_pct:.0}%) / to {VAULT_ADDRESS} {btc_sent:+.6} ({sent_pct:.0}%)"
        );
    } else {
        println!("  Split preview — BTC: no surplus above starting capital yet ({btc_vs_start:+.6})");
    }
    if undead_vs_start > 0.0 {
        let undead_kept = undead_vs_start * (1.0 - ILLUSTRATIVE_SKIM_PCT);
        let undead_sent = undead_vs_start * ILLUSTRATIVE_SKIM_PCT;
        println!(
            "  Split preview (illustrative, no funds moved) — UNDEAD kept {undead_kept:+.2} ({kept_pct:.0}%) / to {VAULT_ADDRESS} {undead_sent:+.2} ({sent_pct:.0}%)"
        );
    } else {
        println!("  Split preview — UNDEAD: no surplus above starting capital yet ({undead_vs_start:+.2})");
    }

    Ok(snap)
}

//============================================================================
//----- div: sending a cut of the surplus to Vault -----------------------------
//============================================================================
/// Pure math, no network — how much BTC/UNDEAD `div` would actually send.
/// Two things this deliberately does NOT do: use the full on-paper surplus
/// (some of it may be locked in open pivots, not actually transferable
/// right now), or trust a caller-supplied percentage without validating
/// it's a real percentage.
fn compute_div_amounts(pct: f64, snap: WalletSnapshot) -> ErrStr<(f64, f64)> {
    if !(0.0..=100.0).contains(&pct) {
        return Err(format!("--pct must be between 0 and 100, got {pct}"));
    }
    let frac = pct / 100.0;

    let btc_surplus = (snap.btc_balance - STARTING_BTC).max(0.0);
    let undead_surplus = (snap.undead_balance - STARTING_UNDEAD).max(0.0);
    // Can only send what's actually free right now — surplus that's
    // currently committed to an open pivot isn't liquid until it closes.
    let btc_sendable = btc_surplus.min(snap.btc_available);
    let undead_sendable = undead_surplus.min(snap.undead_available);

    Ok((btc_sendable * frac, undead_sendable * frac))
}

/// Actually sending funds. Called only from the `div` subcommand, which has
/// no --dry-run option at all (not just a runtime check — the flag isn't
/// in Command::Div's argument list, so clap itself rejects trying to
/// combine them).
async fn divvy_to_vault(wallet_address: &str, registry: &TokenRegistry, pct: f64, snap: WalletSnapshot) -> ErrStr<()> {
    let (btc_to_send, undead_to_send) = compute_div_amounts(pct, snap)?;

    println!("  div: {pct:.1}% of sendable surplus -> Vault ({VAULT_ADDRESS})");
    println!("    BTC    sendable {:.8}   sending {btc_to_send:.8}", snap.btc_available.min((snap.btc_balance - STARTING_BTC).max(0.0)));
    println!("    UNDEAD sendable {:.2}   sending {undead_to_send:.2}", snap.undead_available.min((snap.undead_balance - STARTING_UNDEAD).max(0.0)));

    if btc_to_send <= 0.0 && undead_to_send <= 0.0 {
        println!("  Nothing sendable right now (no surplus, or it's all committed to open pivots) — no transfer made.");
        return Ok(());
    }

    if btc_to_send > 0.0 {
        let (tx_hash, gas_avax) = send_tokens(wallet_address, registry, "BTC", VAULT_ADDRESS, btc_to_send, "TVA_KEYSTORE_PATH").await?;
        println!("  Sent {btc_to_send:.8} BTC to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    }
    if undead_to_send > 0.0 {
        let (tx_hash, gas_avax) = send_tokens(wallet_address, registry, "UNDEAD", VAULT_ADDRESS, undead_to_send, "TVA_KEYSTORE_PATH").await?;
        println!("  Sent {undead_to_send:.2} UNDEAD to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    }

    Ok(())
}

//============================================================================
//----- CLI --------------------------------------------------------------------
//============================================================================
#[derive(Debug, Parser)]
#[command(name = "tva")]
#[command(version = "0.8.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Only meaningful when no subcommand is given (the normal cycle).
    /// `div` has no --dry-run of its own — it's always a real run.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the normal cycle for real, then send `--pct` of the sendable
    /// surplus above starting capital to Vault. The rest ("trim") simply
    /// stays in tvá's own wallet — there's no separate transfer for it.
    Div {
        #[arg(long, default_value_t = DEFAULT_DIV_PCT)]
        pct: f64,
    },
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    match args.command {
        None => {
            run_cycle(args.dry_run).await?;
            Ok(())
        }
        Some(Command::Div { pct }) => {
            let snap = run_cycle(false).await?;
            let wallet_address = wallet_address_from_env("TVA_WALLET_ADDRESS")?;
            let registry = load_token_registry()?;
            divvy_to_vault(&wallet_address, &registry, pct, snap).await
        }
    }
}

//============================================================================
//----- UNIT TESTS -------------------------------------------------------------
//============================================================================
#[cfg(test)]
mod unit_tests {
    use super::*;


    #[test]
    fn test_load_token_registry_has_btc_undead_avax() -> ErrStr<()> {
        let registry = load_token_registry()?;
        for symbol in ["BTC", "UNDEAD", "AVAX"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in tokens.toml");
        }
        Ok(())
    }

    #[test]
    fn test_replay_log_missing_file_is_a_hard_error_not_a_fresh_start() {
        let result = replay_log("/tmp/definitely_does_not_exist.log");
        assert!(result.is_err(), "a missing log file must never be treated as a fresh start — tvá's history is real and pre-existing");
        let msg = result.unwrap_err();
        assert!(msg.contains("history"), "error should make clear this is about missing history, not an empty-is-fine case: '{msg}'");
    }

    #[test]
    fn test_replay_log_open_then_close_leaves_nothing_open_and_totals_gain() -> ErrStr<()> {
        let path = std::env::temp_dir().join("tva_test_open_close.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\tUNDEAD\t500000.00000000\tBTC\t0.00502601\t0.00500000\t0xabc\n\
             1970-01-01 00:33:20\tCLOSE\t1\t1\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, next_pivot, next_close, stats) = replay_log(path_str)?;
        assert!(opens.is_empty(), "pivot 1 was closed, should not appear as open");
        assert_eq!(next_pivot, 2);
        assert_eq!(next_close, 2);
        assert_eq!(stats.total_opens, 1);
        assert_eq!(stats.total_closes, 1);
        assert!((stats.total_gain_undead - 11112.13).abs() < 0.001, "gain should land in the UNDEAD bucket (prim was UNDEAD)");
        assert_eq!(stats.total_gain_btc, 0.0);
        assert!((stats.total_gas_avax - 0.008).abs() < 0.00001, "gas should sum across the OPEN and CLOSE");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn test_replay_log_open_without_close_stays_open() -> ErrStr<()> {
        let path = std::env::temp_dir().join("tva_test_open_only.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\tUNDEAD\t500000.00000000\tBTC\t0.00502601\t0.00500000\t0xabc\n\
             1970-01-01 00:16:40\tOPEN\t2\tBTC\t0.00500000\tUNDEAD\t487122.54000000\t0.00300000\t0xdef\n\
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
    fn test_replay_log_rejects_malformed_line() {
        let path = std::env::temp_dir().join("tva_test_malformed.log");
        std::fs::write(&path, "not\teven\tclose\tto\tvalid\n").unwrap();
        let result = replay_log(path.to_str().unwrap());
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_replay_log_rejects_close_with_no_matching_open() {
        let path = std::env::temp_dir().join("tva_test_orphan_close.log");
        std::fs::write(
            &path,
            "2000\tCLOSE\t99\t1\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
        ).unwrap();
        let result = replay_log(path.to_str().unwrap());
        assert!(result.is_err(), "a CLOSE referencing a pivot_id with no prior OPEN should be a hard error, not silently ignored");
        let _ = std::fs::remove_file(&path);
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
    fn test_compute_div_amounts_rejects_out_of_range_pct() {
        let snap = WalletSnapshot {
            btc_balance: 0.06, btc_committed: 0.0, btc_available: 0.06,
            undead_balance: 5_100_000.0, undead_committed: 0.0, undead_available: 5_100_000.0,
        };
        assert!(compute_div_amounts(-1.0, snap).is_err());
        assert!(compute_div_amounts(100.01, snap).is_err());
        assert!(compute_div_amounts(0.0, snap).is_ok());
        assert!(compute_div_amounts(100.0, snap).is_ok());
    }

    #[test]
    fn test_compute_div_amounts_uses_full_surplus_when_all_free() {
        // 0.01 BTC and 100,000 UNDEAD surplus above starting capital, all free.
        let snap = WalletSnapshot {
            btc_balance: 0.06, btc_committed: 0.0, btc_available: 0.06,
            undead_balance: 5_100_000.0, undead_committed: 0.0, undead_available: 5_100_000.0,
        };
        let (btc, undead) = compute_div_amounts(25.0, snap).unwrap();
        assert!((btc - 0.0025).abs() < 1e-9, "25% of 0.01 BTC surplus should be 0.0025, got {btc}");
        assert!((undead - 25_000.0).abs() < 1e-6, "25% of 100,000 UNDEAD surplus should be 25,000, got {undead}");
    }

    #[test]
    fn test_compute_div_amounts_caps_at_available_when_surplus_is_committed() {
        // Balance is 0.01 BTC above starting capital, but only 0.002 of that
        // is actually free — the rest is tied up in an open pivot.
        let snap = WalletSnapshot {
            btc_balance: 0.06, btc_committed: 0.008, btc_available: 0.002,
            undead_balance: 5_000_000.0, undead_committed: 0.0, undead_available: 5_000_000.0,
        };
        let (btc, undead) = compute_div_amounts(100.0, snap).unwrap();
        assert!((btc - 0.002).abs() < 1e-9, "should be capped at the free 0.002, not the full 0.01 surplus, got {btc}");
        assert_eq!(undead, 0.0, "balance exactly at starting capital means no surplus at all");
    }

    #[test]
    fn test_compute_div_amounts_zero_when_below_starting_capital() {
        // Temporarily below starting capital (e.g. mid-pivot) — never negative.
        let snap = WalletSnapshot {
            btc_balance: 0.045, btc_committed: 0.0, btc_available: 0.045,
            undead_balance: 5_000_000.0, undead_committed: 0.0, undead_available: 5_000_000.0,
        };
        let (btc, undead) = compute_div_amounts(50.0, snap).unwrap();
        assert_eq!(btc, 0.0);
        assert_eq!(undead, 0.0);
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
}

//============================================================================
//----- FUNCTIONAL TESTS -------------------------------------------------------
//============================================================================
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };


    create_testing!("quiz01::a_tva");

    run!("wallet_balance_btc", " (real BTC.b read against dedicated test wallet, read-only)", {
        let registry = load_token_registry()?;
        let balance = now(wallet_balance(
            "0x6700bD7EAE41434f566e48738813fC585B95669a",
            "BTC",
            &registry,
        ))?;
        println!("\ttest wallet BTC balance: {balance:.8}");
    });

    run!("wallet_balance_undead", " (real UNDEAD read against dedicated test wallet, read-only)", {
        let registry = load_token_registry()?;
        let balance = now(wallet_balance(
            "0x6700bD7EAE41434f566e48738813fC585B95669a",
            "UNDEAD",
            &registry,
        ))?;
        println!("\ttest wallet UNDEAD balance: {balance:.2}");
    });

    run!("live_quote_undead_to_btc", " (real KyberSwap route, read-only, 500000 UNDEAD -> BTC)", {
        let registry = load_token_registry()?;
        let quote = now(live_quote(&registry, "UNDEAD", "BTC", 500_000.0))?;
        println!("\t500000 UNDEAD -> {:.8} BTC right now (router: {})", quote.amount_out, quote.router_address);
    });

    run!("live_quote_btc_to_undead", " (real KyberSwap route, read-only, 0.005 BTC -> UNDEAD)", {
        let registry = load_token_registry()?;
        let quote = now(live_quote(&registry, "BTC", "UNDEAD", 0.005))?;
        println!("\t0.005 BTC -> {:.2} UNDEAD right now (router: {})", quote.amount_out, quote.router_address);
    });

    run!("cycle_dry_run", " (real balances + quotes, never touches keystore)", {
        now(run_cycle(true))?;
        println!("\tdry-run cycle completed without touching the keystore");
    });
}
