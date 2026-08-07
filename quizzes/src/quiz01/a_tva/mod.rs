use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        file_utils::lines_from_file,
        parse_args_add_banner,
};
use trading::auto_trading::{
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
const STARTING_UNDEAD: f64 = 5_000_000.0;
const STARTING_BTC: f64 = 0.05;
const ILLUSTRATIVE_SKIM_PCT: f64 = 0.50;
const VAULT_ADDRESS: &str = "0xe25835EF625ecE064d899844e17Ea59742369A92";
const DEFAULT_DIV_PCT: f64 = 25.0;
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
    let ans = format!(
        "{:.8}\t{:.8}\t{:.8}\t{:.2}\t{:.2}\t{:.2}\t{:+.8}\t{:+.2}\t{:.8}\t{:.6}\t{:.6}",
        snap.btc_balance, snap.btc_committed, snap.btc_available,
        snap.undead_balance, snap.undead_committed, snap.undead_available,
        cum.total_gain_btc, cum.total_gain_undead, cum.total_gas_avax,
        cum.avg_roi(), cum.avg_apr(),
    );
    ans
}

/// Every row — OPEN or CLOSE — uses this exact column layout. Only
/// close_id/gain/roi/apr are ever blank (OPEN doesn't have them yet);
/// everything else, including prim_amount, is filled on both row types
/// since it's always known regardless of which kind of row this is.
fn log_row(
    kind: &str,
    pivot_id: u32,
    close_id: Option<u32>,
    prim: &str,
    proper: &str,
    prim_amount: f64,
    proper_amount: f64,
    gain: Option<f64>,
    roi: Option<f64>,
    apr: Option<f64>,
    gas_avax: f64,
    tx_hash: &str,
    snap: &WalletSnapshot,
    cum: &CumulativeStats,
) {
    let close_id_s = close_id.map(|v| v.to_string()).unwrap_or_default();
    let gain_s = gain.map(|v| format!("{v:+.8}")).unwrap_or_default();
    let roi_s = roi.map(|v| format!("{v:.6}")).unwrap_or_default();
    let apr_s = apr.map(|v| format!("{v:.6}")).unwrap_or_default();
    append_log(&format!(
        "{}\t{kind}\t{pivot_id}\t{close_id_s}\t{prim}\t{proper}\t{prim_amount:.8}\t{proper_amount:.8}\t{gain_s}\t{roi_s}\t{apr_s}\t{gas_avax:.8}\t{tx_hash}\t{}",
        log_ts(now_ts()), snapshot_and_cumulative_columns(snap, cum)
    ));
}

fn log_open(pivot_id: u32, prim: &str, prim_amount: f64, proper: &str, proper_amount: f64, gas_avax: f64, tx_hash: &str, snap: &WalletSnapshot, cum: &CumulativeStats) {
    log_row("OPEN", pivot_id, None, prim, proper, prim_amount, proper_amount, None, None, None, gas_avax, tx_hash, snap, cum);
}

fn log_close(pivot_id: u32, close_id: u32, prim: &str, prim_amount: f64, proper: &str, proper_amount: f64, gain: f64, roi: f64, apr: f64, gas_avax: f64, tx_hash: &str, snap: &WalletSnapshot, cum: &CumulativeStats) {
    log_row("CLOSE", pivot_id, Some(close_id), prim, proper, prim_amount, proper_amount, Some(gain), Some(roi), Some(apr), gas_avax, tx_hash, snap, cum);
}

fn replay_log(path: &str) -> ErrStr<(Vec<OpenPivot>, u32, u32, CumulativeStats)> {
    let lines = lines_from_file(path).map_err(|e| format!(
        "Where is tvá's history? Could not open trade log at '{path}': {e}. \
         This program is wired to real, pre-existing trade history — a missing \
         log file means the path is wrong, not that this is a fresh start. Refusing to run."
    ))?;

    let mut open_by_id: HashMap<u32, OpenPivot> = HashMap::new();
    let mut max_pivot_id: u32 = 0;
    let mut max_close_id: u32 = 0;
    let mut stats = CumulativeStats::default();

    for (line_no, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let kind = *fields.get(1).unwrap_or(&"");

        if kind == "CHECK" {
            continue;
        }

        if fields.len() < 13 {
            return Err(format!("malformed line at {path}:{}: too few columns: '{line}'", line_no + 1));
        }
        let ts: u64 = parse_log_ts(fields[0])
            .map_err(|e| format!("{e} at {path}:{}: '{line}'", line_no + 1))?;
        let pivot_id: u32 = fields[2].parse()
            .map_err(|_| format!("bad pivot_id at {path}:{}: '{line}'", line_no + 1))?;
        let prim = fields[4];
        let proper = fields[5];
        let prim_amount: f64 = fields[6].parse()
            .map_err(|_| format!("bad prim_amount at {path}:{}: '{line}'", line_no + 1))?;
        let proper_amount: f64 = fields[7].parse()
            .map_err(|_| format!("bad proper_amount at {path}:{}: '{line}'", line_no + 1))?;
        let gas_avax: f64 = fields[11].parse()
            .map_err(|_| format!("bad gas_avax at {path}:{}: '{line}'", line_no + 1))?;

        match kind {
            "OPEN" => {
                max_pivot_id = max_pivot_id.max(pivot_id);
                stats.total_opens += 1;
                stats.total_gas_avax += gas_avax;
                open_by_id.insert(pivot_id, OpenPivot {
                    pivot_id,
                    opened_at: ts,
                    prim: prim.to_string(),
                    prim_amount,
                    proper: proper.to_string(),
                    proper_amount,
                });
            }
            "CLOSE" => {
                let close_id: u32 = fields[3].parse()
                    .map_err(|_| format!("bad close_id at {path}:{}: '{line}'", line_no + 1))?;
                let gain: f64 = fields[8].parse()
                    .map_err(|_| format!("bad gain at {path}:{}: '{line}'", line_no + 1))?;
                let roi: f64 = fields[9].parse()
                    .map_err(|_| format!("bad roi at {path}:{}: '{line}'", line_no + 1))?;
                let apr: f64 = fields[10].parse()
                    .map_err(|_| format!("bad apr at {path}:{}: '{line}'", line_no + 1))?;

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
            other => return Err(format!("unrecognized log line type '{other}' at {path}:{}: '{line}'", line_no + 1)),
        }
    }

    let still_open: Vec<OpenPivot> = open_by_id.into_values().collect();
    Ok((still_open, max_pivot_id + 1, max_close_id + 1, stats))
}

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
    debug: bool,
) -> ErrStr<AttemptOutcome> {
    let quote = live_quote(registry, from_symbol, to_symbol, amount).await?;
    if quote.amount_out <= min_floor {
        return Ok(AttemptOutcome::NotCleared);
    }

    if dry_run {
        return Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out: quote.amount_out });
    }

    let balance_before = wallet_balance(wallet_address, to_symbol, registry).await?;
    let (tx_hash, gas_avax) = execute_trade(wallet_address, registry, from_symbol, to_symbol, amount, min_floor, SLIPPAGE_BPS, "TVA_KEYSTORE_PATH", debug).await?;
    let balance_after = wallet_balance(wallet_address, to_symbol, registry).await?;
    let actual_received = balance_after - balance_before;

    Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax })
}

pub async fn run_cycle(dry_run: bool, debug: bool) -> ErrStr<WalletSnapshot> {
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
    // parse_args_add_banner! already printed the name+version banner on
    // startup — this used to reprint it a second time. The rest of this
    // intro (open-pivot count, wallet address) is detail, not signal, so
    // it's --debug-only now; the trade lines below carry the real story.
    if debug {
        println!();
        println!("tvá — {}{mode_tag} — {} open pivot(s), wallet {wallet_address}", human_ts(now_ts()), open_pivots.len());
        println!();
    }

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
            pivot.proper_amount, pivot.prim_amount, dry_run, debug,
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
                log_close(pivot.pivot_id, next_close_id, &pivot.prim, pivot.prim_amount, &pivot.proper, actual_received, gain, roi, apr, gas_avax, &tx_hash, &snap, &running_stats);
                next_close_id += 1;
                closed_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                let gain = quoted_amount_out - pivot.prim_amount;
                let roi = gain / pivot.prim_amount;
                println!(
                    "  WOULD CLOSE  #{:<4} {:.8} {} -> ~{:.4} {}   est. gain {:+.4} {}   est. roi {:.2}%   (dry run, no funds moved)",
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
    if debug {
        println!();
        println!(
            "Wallet's Status = BTC {:.6} in wallet ({:.6} committed, {:.6} available) | UNDEAD {:.2} in wallet ({:.2} committed, {:.2} available)",
            snap.btc_balance, snap.btc_committed, snap.btc_available, snap.undead_balance, snap.undead_committed, snap.undead_available
        );
        println!();
    }

    let mut opened_something = false;
    let mut skipped_open_reasons: Vec<String> = Vec::new();

    if snap.btc_available > BTC_TRADE_AMOUNT {
        let attempted_id = next_pivot_id;
        match attempt_trade_with_actual_amount(
            &wallet_address, &registry, "BTC", "UNDEAD", BTC_TRADE_AMOUNT, NO_REAL_FLOOR, dry_run, debug,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.8} BTC -> {actual_received:.2} UNDEAD   gas {gas_avax:.5} AVAX");
                committed_undead += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
                log_open(attempted_id, "BTC", BTC_TRADE_AMOUNT, "UNDEAD", actual_received, gas_avax, &tx_hash, &snap, &running_stats);
                opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.8} BTC -> ~{quoted_amount_out:.2} UNDEAD   (dry run, no funds moved)");
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
            &wallet_address, &registry, "UNDEAD", "BTC", UNDEAD_TRADE_AMOUNT, NO_REAL_FLOOR, dry_run, debug,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> {actual_received:.8} BTC   gas {gas_avax:.5} AVAX");
                committed_btc += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                snap = wallet_snapshot(&wallet_address, &registry, committed_btc, committed_undead).await?;
                log_open(next_pivot_id, "UNDEAD", UNDEAD_TRADE_AMOUNT, "BTC", actual_received, gas_avax, &tx_hash, &snap, &running_stats);
                opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> ~{quoted_amount_out:.8} BTC   (dry run, no funds moved)");
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
        "Totals = {} closes, {} opens   gain BTC {:+.8}   gain UNDEAD {:+.4}   gas {:.5} AVAX   avg roi {:.2}%   avg apr {:.2}%",
        running_stats.total_closes, running_stats.total_opens,
        running_stats.total_gain_btc, running_stats.total_gain_undead, running_stats.total_gas_avax,
        running_stats.avg_roi() * 100.0, running_stats.avg_apr() * 100.0
    );

    if running_stats.total_gain_btc < 0.0 || running_stats.total_gain_undead < 0.0 {
        println!(
            "  \u{26A0} WARNING: realized cumulative gain is negative — this is NOT a timing artifact, \
             closes are actually losing money. BTC {:+.8}   UNDEAD {:+.4}",
            running_stats.total_gain_btc, running_stats.total_gain_undead
        );
    }

    let btc_vs_start = snap.btc_balance - STARTING_BTC;
    let undead_vs_start = snap.undead_balance - STARTING_UNDEAD;
    if debug {
        println!(
            "Vs starting capital ({STARTING_UNDEAD} UNDEAD / {STARTING_BTC} BTC entrusted): BTC {btc_vs_start:+.8}   UNDEAD {undead_vs_start:+.4}"
        );
        println!();

        let kept_pct = (1.0 - ILLUSTRATIVE_SKIM_PCT) * 100.0;
        let sent_pct = ILLUSTRATIVE_SKIM_PCT * 100.0;
        if btc_vs_start > 0.0 {
            let btc_kept = btc_vs_start * (1.0 - ILLUSTRATIVE_SKIM_PCT);
            let btc_sent = btc_vs_start * ILLUSTRATIVE_SKIM_PCT;
            println!(
                "  Split preview (illustrative, no funds moved) — BTC kept {btc_kept:+.8} ({kept_pct:.0}%) and ({sent_pct:.0}%) to Vault {btc_sent:+.8}"
            );
        } else {
            println!("  Split preview — BTC: no surplus above starting capital yet ({btc_vs_start:+.8})");
        }
        if undead_vs_start > 0.0 {
            let undead_kept = undead_vs_start * (1.0 - ILLUSTRATIVE_SKIM_PCT);
            let undead_sent = undead_vs_start * ILLUSTRATIVE_SKIM_PCT;
            println!(
                "  Split preview (illustrative, no funds moved) — UNDEAD kept {undead_kept:+.4} ({kept_pct:.0}%) and ({sent_pct:.0}%) to Vault {undead_sent:+.4}"
            );
        } else {
            println!("  Split preview — UNDEAD: no surplus above starting capital yet ({undead_vs_start:+.4})");
        }
    }

    Ok(snap)
}

//============================================================================
//----- div: sending a cut of the surplus to Vault -----------------------------
//============================================================================
fn compute_div_amounts(pct: f64, snap: WalletSnapshot) -> ErrStr<(f64, f64)> {
    if !(0.0..=100.0).contains(&pct) {
        return Err(format!("--pct must be between 0 and 100, got {pct}"));
    }
    let frac = pct / 100.0;

    let btc_surplus = (snap.btc_balance - STARTING_BTC).max(0.0);
    let undead_surplus = (snap.undead_balance - STARTING_UNDEAD).max(0.0);
    let btc_sendable = btc_surplus.min(snap.btc_available);
    let undead_sendable = undead_surplus.min(snap.undead_available);

    Ok((btc_sendable * frac, undead_sendable * frac))
}

async fn divvy_to_vault(wallet_address: &str, registry: &TokenRegistry, pct: f64, snap: WalletSnapshot, dry_run: bool, debug: bool) -> ErrStr<()> {
    let (btc_to_send, undead_to_send) = compute_div_amounts(pct, snap)?;
    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };

    println!("  div{mode_tag}: {pct:.1}% of sendable surplus -> Vault ({VAULT_ADDRESS})");
    println!("    BTC    sendable {:.8}   sending {btc_to_send:.8}", snap.btc_available.min((snap.btc_balance - STARTING_BTC).max(0.0)));
    println!("    UNDEAD sendable {:.4}   sending {undead_to_send:.4}", snap.undead_available.min((snap.undead_balance - STARTING_UNDEAD).max(0.0)));

    if btc_to_send <= 0.0 && undead_to_send <= 0.0 {
        println!("  Nothing sendable right now (no surplus, or it's all committed to open pivots) — no transfer made.");
        return Ok(());
    }

    if dry_run {
        if btc_to_send > 0.0 {
            println!("  Would send {btc_to_send:.8} BTC to Vault. (dry run, no funds moved)");
        }
        if undead_to_send > 0.0 {
            println!("  Would send {undead_to_send:.4} UNDEAD to Vault. (dry run, no funds moved)");
        }
        return Ok(());
    }

    if btc_to_send > 0.0 {
        let (tx_hash, gas_avax) = send_tokens(wallet_address, registry, "BTC", VAULT_ADDRESS, btc_to_send, "TVA_KEYSTORE_PATH", debug).await?;
        println!("  Sent {btc_to_send:.8} BTC to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    }
    if undead_to_send > 0.0 {
        let (tx_hash, gas_avax) = send_tokens(wallet_address, registry, "UNDEAD", VAULT_ADDRESS, undead_to_send, "TVA_KEYSTORE_PATH", debug).await?;
        println!("  Sent {undead_to_send:.4} UNDEAD to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    }

    Ok(())
}

//============================================================================
//----- CLI --------------------------------------------------------------------
//============================================================================
#[derive(Debug, Parser)]
#[command(name = "tva")]
#[command(bin_name = "tva")]
#[command(version = "0.13.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, default_value_t = false)]
    dry_run: bool,

    #[arg(long, default_value_t = false)]
    debug: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    Div {
        #[arg(long, default_value_t = DEFAULT_DIV_PCT)]
        pct: f64,
    },
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    match args.command {
        None => {
            run_cycle(args.dry_run, args.debug).await?;
            Ok(())
        }
        Some(Command::Div { pct }) => {
            let snap = run_cycle(args.dry_run, args.debug).await?;
            let wallet_address = wallet_address_from_env("TVA_WALLET_ADDRESS")?;
            let registry = load_token_registry()?;
            divvy_to_vault(&wallet_address, &registry, pct, snap, args.dry_run, args.debug).await
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
            "1970-01-01 00:16:40\tOPEN\t1\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n\
             1970-01-01 00:33:20\tCLOSE\t1\t1\tUNDEAD\tBTC\t500000.00000000\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
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
            "1970-01-01 00:16:40\tOPEN\t1\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n\
             1970-01-01 00:16:40\tOPEN\t2\t\tBTC\tUNDEAD\t0.00500000\t487122.54000000\t\t\t\t0.00300000\t0xdef\n\
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
            "2026-01-01 00:00:00\tCLOSE\t99\t1\tUNDEAD\tBTC\t500000.00000000\t511112.13000000\t11112.13000000\t0.022224\t167.780000\t0.00300000\t0xdef\n",
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
        now(run_cycle(true, false))?;
        println!("\tdry-run cycle completed without touching the keystore");
    });
}
