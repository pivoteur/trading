use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use clap::{Parser, Subcommand};
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        parse_args_add_banner,
};
use libs::{
        fetchers::calls::fetch_calls,
        types::calls::Call,
};
use trading::auto_trading::{
                TokenRegistry, parse_token_registry, token_entry,
                wallet_address_from_env, wallet_balance, kyber_swap, execute_trade,
                balance_snapshot, BalanceSnapshot,
                AttemptOutcome, attempt_trade_with_actual_amount,
                biggest_first, now_ts, replay_log, log_open, log_close,
                UNDEAD, NO_REAL_FLOOR,
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================
/// address is ever inferred or looked up live, for any subcommand.
/// contract addresses are hardcoded in tokens.toml, which is compiled into the binary
const TOKENS_TOML: &str = include_str!("tokens.toml");

pub fn load_token_registry() -> ErrStr<TokenRegistry> {
    parse_token_registry(TOKENS_TOML)
}

//============================================================================
//----- Constants ---------------------------------------------------------------
//============================================================================
const DEFAULT_SLIPPAGE_BPS: u16 = 50;
/// Env var holding the path to arbitrage's own encrypted keystore file —
/// kept as one named constant rather than repeating the literal at each
/// call site.
const KEYSTORE_PATH_VAR: &str = "KEYSTORE_PATH";

//============================================================================
//----- Per-Pool Trade Log — path & header --------------------------------------
//============================================================================
/// Every pool gets its own log file, named after its non-UNDEAD token,
/// living alongside this binary's other data — e.g.
/// `data/avax_undead-trades.log`, `data/paxg_undead-trades.log`. One file
/// per pool keeps `new` purely additive: bootstrapping a pool never
/// touches any other pool's history, and the default survey discovers
/// pools by looking for these files rather than from a hardcoded list.
fn pool_log_path(token: &str) -> String {
    let ans = format!(
        "{}/data/{}_undead-trades.log",
        env!("CARGO_MANIFEST_DIR"),
        token.to_lowercase()
    );
    ans
}

const LOG_HEADER: &str = "timestamp\tkind\tpivot_id\tclose_id\tprim\tproper\tprim_amount\tproper_amount\tgain\troi\tapr\tgas_avax\ttx_hash\tasset_balance\tasset_committed\tasset_available\tundead_balance\tundead_committed\tundead_available\ttotal_gain_asset\ttotal_gain_undead\ttotal_gas_avax\tavg_roi\tavg_apr";

/// Finds every pool that has ever been opened by scanning this binary's
/// data/ directory for per-pool log files — "go through any existing
/// logs," literally — rather than a hardcoded list that would silently
/// drift out of sync every time `new` bootstraps another pool.
fn discover_pools() -> ErrStr<Vec<String>> {
    let data_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    let mut pools = Vec::new();
    let entries = match std::fs::read_dir(data_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(pools), // no pools opened yet
        Err(e) => return Err(format!("Could not read {data_dir}: {e}")),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read a directory entry in {data_dir}: {e}"))?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if let Some(token) = file_name.strip_suffix("_undead-trades.log") {
            pools.push(token.to_uppercase());
        }
    }
    pools.sort();
    Ok(pools)
}

//============================================================================
//----- One Pool's Cycle: close what can close ----------------------------------
//============================================================================
/// Per-pool amount to commit when opening a fresh position during the
/// default survey. Deliberately empty for every pool right now — sizing
/// real capital automatically is a decision that needs an explicit human
/// call, not something to guess at from wallet balance. A pool with no
/// entry here just gets skipped on the open side (closes still run);
/// fill in real amounts here per pool when ready for the survey to open
/// positions on its own.
fn open_trade_amount(_token: &str) -> Option<f64> {
    None
}
// ------
struct PoolCycleResult {
    closed_something: bool,
    opened_something: bool,
    /// This pool's final balance/committed/available state for this cycle
    /// — feeds the wallet-health table `run_survey` prints at the end, at
    /// no extra cost to the caller since it's fetched here regardless.
    health: BalanceSnapshot,
}
// -----
async fn run_pool_cycle(
    wallet_address: &str,
    registry: &TokenRegistry,
    token: &str,
    dry_run: bool,
    debug: bool,
) -> ErrStr<PoolCycleResult> {
    let path = pool_log_path(token);
    let (open_pivots, _next_pivot_id, mut next_close_id, opening_stats) = replay_log(&path)?;
    let open_pivots = biggest_first(open_pivots);

    println!();
    println!("== {token} <-> UNDEAD ==");

    let mut committed_token: f64 = open_pivots.iter().filter(|p| p.proper == token).map(|p| p.proper_amount).sum();
    let mut committed_undead: f64 = open_pivots.iter().filter(|p| p.proper == UNDEAD).map(|p| p.proper_amount).sum();
    let mut running_stats = opening_stats;
    let mut closed_something = false;

    for pivot in open_pivots {
        match attempt_trade_with_actual_amount(
            wallet_address, registry, &pivot.proper, &pivot.prim,
            pivot.proper_amount, pivot.prim_amount, DEFAULT_SLIPPAGE_BPS, KEYSTORE_PATH_VAR, dry_run, debug,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                let gain = actual_received - pivot.prim_amount;
                let roi = gain / pivot.prim_amount;
                let days_held = (now_ts().saturating_sub(pivot.opened_at)) as f64 / 86_400.0;
                let apr = if days_held > 0.0 { roi * 365.0 / days_held } else { 0.0 };
                println!(
                    "  CLOSED  #{:<4} {:.6} {} -> {:.6} {}   gain {:+.6} {}   roi {:.2}%   apr {:.1}%   gas {:.5} AVAX",
                    pivot.pivot_id, pivot.proper_amount, pivot.proper, actual_received, pivot.prim,
                    gain, pivot.prim, roi * 100.0, apr * 100.0, gas_avax
                );

                if pivot.proper == token {
                    committed_token -= pivot.proper_amount;
                } else if pivot.proper == UNDEAD {
                    committed_undead -= pivot.proper_amount;
                }
                running_stats.total_closes += 1;
                running_stats.total_gas_avax += gas_avax;
                running_stats.roi_sum += roi;
                running_stats.apr_sum += apr;
                if pivot.prim == UNDEAD {
                    running_stats.total_gain_undead += gain;
                } else {
                    running_stats.total_gain_asset += gain;
                }

                let snap = balance_snapshot(wallet_address, registry, token, committed_token, committed_undead).await?;
                log_close(&path, Some(LOG_HEADER), pivot.pivot_id, next_close_id, &pivot.prim, pivot.prim_amount, &pivot.proper, actual_received, gain, roi, apr, gas_avax, &tx_hash, &snap, &running_stats);
                next_close_id += 1;
                closed_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                let gain = quoted_amount_out - pivot.prim_amount;
                let roi = gain / pivot.prim_amount;
                println!(
                    "  WOULD CLOSE  #{:<4} {:.8} {} -> ~{:.6} {}   est. gain {:+.6} {}   est. roi {:.2}%   (dry run, no funds moved)",
                    pivot.pivot_id, pivot.proper_amount, pivot.proper, quoted_amount_out, pivot.prim,
                    gain, pivot.prim, roi * 100.0
                );
                closed_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => {}
            Err(e) => println!("  ! close attempt for pivot #{} failed, staying open: {e}", pivot.pivot_id),
        }
    }

    if !closed_something {
        println!("  Nothing to close this cycle.");
    }

    if open_trade_amount(token).is_none() {
        println!("  (no configured open amount for {token} yet — opens skipped, closes only)");
    }

    if running_stats.total_gain_asset < 0.0 || running_stats.total_gain_undead < 0.0 {
        println!(
            "  \u{26A0} WARNING: realized cumulative gain is negative for {token} — this is NOT a \
             timing artifact, closes are actually losing money. {token} {:+.6}   UNDEAD {:+.2}",
            running_stats.total_gain_asset, running_stats.total_gain_undead
        );
    }

    println!(
        "  Totals = {} closes, {} opens   gain {token} {:+.6}   gain UNDEAD {:+.2}   gas {:.5} AVAX   avg roi {:.2}%   avg apr {:.2}%",
        running_stats.total_closes, running_stats.total_opens,
        running_stats.total_gain_asset, running_stats.total_gain_undead, running_stats.total_gas_avax,
        running_stats.avg_roi() * 100.0, running_stats.avg_apr() * 100.0
    );

    // Fresh read for the wallet-health table: committed_token/committed_undead
    // reflect this cycle's closes, so this is the pool's true post-cycle
    // state — not whatever the last pivot happened to snapshot, which is
    // absent entirely on a cycle where nothing closed.
    let health = balance_snapshot(wallet_address, registry, token, committed_token, committed_undead).await?;

    Ok(PoolCycleResult { closed_something, opened_something: false, health })
}

/// No subcommand = the default full survey: walk every pool that has an
/// existing log, close what can close in each.
/// Opens are wired up per-pool via open_trade_amount but every pool
/// currently returns None there, so this only ever closes until amounts
/// are configured — see the note on open_trade_amount above.
pub async fn run_survey(dry_run: bool, debug: bool) -> ErrStr<()> {
    let wallet_address = wallet_address_from_env("WALLET_ADDRESS")?;
    let registry = load_token_registry()?;
    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };

    println!("arbitrage — full survey{mode_tag} — wallet {wallet_address}");

    let pools = discover_pools()?;
    if pools.is_empty() {
        println!("No pools opened yet — run `arbitrage new <TOKEN> <amount>` to bootstrap one.");
    }

    let mut any_closed = false;
    let mut any_opened = false;
    let mut healths: Vec<(String, BalanceSnapshot)> = Vec::new();
    for token in &pools {
        if token_entry(&registry, token).is_err() {
            println!();
            println!("== {token} <-> UNDEAD ==");
            println!("  SKIPPED: '{token}' has a log but is no longer in tokens.toml");
            continue;
        }
        let result = run_pool_cycle(&wallet_address, &registry, token, dry_run, debug).await?;
        any_closed |= result.closed_something;
        any_opened |= result.opened_something;
        healths.push((token.clone(), result.health));
    }

    if !pools.is_empty() {
        println!();
        if !any_closed && !any_opened {
            println!("Nothing to close or open across {} pool(s) this cycle — see ya next time!", pools.len());
        }
    }

    // Always printed — even (especially) with zero pools open, since that's
    // exactly when "what do I have available to bootstrap a pool with?" is
    // the most useful question to answer.
    print_wallet_health(&wallet_address, &registry, &healths).await?;

    Ok(())
}

//============================================================================
//----- Wallet Health: default-survey summary table ------------------------
//============================================================================
/// Printed automatically as the last thing `run_survey` does — read-only,
/// always shown whether or not `--dry-run` is set, since nothing here
/// touches the keystore or sends a tx. Bootstrapped pools get their full
/// balance/committed/available breakdown (already fetched this cycle
/// inside `run_pool_cycle`, so this costs nothing extra); every other
/// tokens.toml entry gets a plain balance-only read since there's no pool
/// to commit against yet. UNDEAD is the one asset every pool draws from,
/// so instead of repeating its balance once per pool (same number, just
/// different committed/available each time) it collapses into a single
/// totals line — that's the number that actually answers "how much is
/// available for everything wired into arbitrage."
async fn print_wallet_health(
    wallet_address: &str,
    registry: &TokenRegistry,
    pool_healths: &[(String, BalanceSnapshot)],
) -> ErrStr<()> {
    println!();
    println!("== Wallet Health ==");

    for (token, snap) in pool_healths {
        println!(
            "  {:<8} balance {:.6}   committed {:.6}   available {:.6}",
            token, snap.asset_balance, snap.asset_committed, snap.asset_available
        );
    }

    let pool_tokens: std::collections::HashSet<&str> =
        pool_healths.iter().map(|(t, _)| t.as_str()).collect();
    let mut other_tokens: Vec<&str> = registry
        .keys()
        .map(|s| s.as_str())
        .filter(|s| *s != UNDEAD && !pool_tokens.contains(s))
        .collect();
    other_tokens.sort();

    for token in &other_tokens {
        let balance = wallet_balance(wallet_address, token, registry).await?;
        println!("  {:<8} balance {:.6}   (not yet bootstrapped into a pool)", token, balance);
    }

    println!("  --");
    if !pool_healths.is_empty() {
        let undead_balance = pool_healths[0].1.undead_balance;
        let total_committed: f64 = pool_healths.iter().map(|(_, s)| s.undead_committed).sum();
        println!(
            "  {:<8} balance {:.6}   total committed {:.6} (across {} pool{})   available {:.6}",
            UNDEAD, undead_balance, total_committed, pool_healths.len(),
            if pool_healths.len() == 1 { "" } else { "s" },
            undead_balance - total_committed
        );
    } else {
        // No pools yet means nothing's committed anywhere — still show the
        // real UNDEAD balance so "what's available to bootstrap with" has
        // an actual answer instead of UNDEAD silently vanishing from the
        // table entirely.
        let undead_balance = wallet_balance(wallet_address, UNDEAD, registry).await?;
        println!(
            "  {:<8} balance {:.6}   (no pools open yet — nothing committed)",
            UNDEAD, undead_balance
        );
    }

    Ok(())
}

//============================================================================
//----- new: bootstrap a brand-new pool with a matching pair of opens -----------
//============================================================================
/// Opens a brand-new TOKEN<->UNDEAD pool with two pivots in one run:
/// TOKEN -> UNDEAD for the amount given, then UNDEAD -> TOKEN using
/// exactly the UNDEAD pivot one actually received — that's already the
/// live market's answer for what that TOKEN amount is worth right now, so
/// re-quoting independently would just ask the same question again (and
/// could drift from what pivot one actually executed at). Both pivots land
/// in the new pool's log as their own OPEN pivots.
pub async fn run_new(token: &str, amount: f64, slippage_bps: u16, dry_run: bool, debug: bool) -> ErrStr<()> {
    let token = token.to_uppercase();
    if amount <= 0.0 {
        return Err("amount must be greater than zero".to_string());
    }

    let wallet_address = wallet_address_from_env("WALLET_ADDRESS")?;
    let registry = load_token_registry()?;

    if token_entry(&registry, &token).is_err() {
        return Err(format!(
            "'{token}' is not in tokens.toml — add its address and decimals there first. No funds moved."
        ));
    }

    let path = pool_log_path(&token);
    if Path::new(&path).exists() {
        return Err(format!(
            "{path} already exists — 'new' is for bootstrapping a pool for the first time \
             only. If you meant to open another position in an existing pool, that's not \
             wired up yet; nothing was done. No funds moved."
        ));
    }

    let (_, mut next_pivot_id, _, mut running_stats) = replay_log(&path)?; // fresh: (vec![], 1, 1, default)

    println!("Bootstrapping new pool: {token} <-> UNDEAD{}", if dry_run { " [DRY RUN]" } else { "" });

    // Pivot 1: TOKEN -> UNDEAD
    let undead_received = match attempt_trade_with_actual_amount(
        &wallet_address, &registry, &token, UNDEAD, amount, NO_REAL_FLOOR, slippage_bps, KEYSTORE_PATH_VAR, dry_run, debug,
    ).await? {
        AttemptOutcome::Executed { tx_hash, actual_received, gas_avax } => {
            println!("  OPENED  #{next_pivot_id:<4} {amount:.8} {token} -> {actual_received:.2} UNDEAD   gas {gas_avax:.5} AVAX");
            running_stats.total_opens += 1;
            running_stats.total_gas_avax += gas_avax;
            let snap = balance_snapshot(&wallet_address, &registry, &token, 0.0, actual_received).await?;
            log_open(&path, Some(LOG_HEADER), next_pivot_id, &token, amount, UNDEAD, actual_received, gas_avax, &tx_hash, &snap, &running_stats);
            next_pivot_id += 1;
            actual_received
        }
        AttemptOutcome::DryRunWouldClear { quoted_amount_out } => {
            println!("  WOULD OPEN  #{next_pivot_id:<4} {amount:.8} {token} -> ~{quoted_amount_out:.2} UNDEAD   (dry run, no funds moved)");
            next_pivot_id += 1;
            quoted_amount_out
        }
        AttemptOutcome::NotCleared => {
            return Err(format!("unexpected: {token}->UNDEAD quote didn't clear the near-zero floor — check the pool. No funds moved."));
        }
    };

    // Pivot 2: UNDEAD -> TOKEN, using exactly what pivot 1 returned — not a fresh quote.
    match attempt_trade_with_actual_amount(
        &wallet_address, &registry, UNDEAD, &token, undead_received, NO_REAL_FLOOR, slippage_bps, KEYSTORE_PATH_VAR, dry_run, debug,
    ).await? {
        AttemptOutcome::Executed { tx_hash, actual_received, gas_avax } => {
            println!("  OPENED  #{next_pivot_id:<4} {undead_received:.2} UNDEAD -> {actual_received:.8} {token}   gas {gas_avax:.5} AVAX");
            running_stats.total_opens += 1;
            running_stats.total_gas_avax += gas_avax;
            let snap = balance_snapshot(&wallet_address, &registry, &token, actual_received, undead_received).await?;
            log_open(&path, Some(LOG_HEADER), next_pivot_id, UNDEAD, undead_received, &token, actual_received, gas_avax, &tx_hash, &snap, &running_stats);
        }
        AttemptOutcome::DryRunWouldClear { quoted_amount_out } => {
            println!("  WOULD OPEN  #{next_pivot_id:<4} {undead_received:.2} UNDEAD -> ~{quoted_amount_out:.8} {token}   (dry run, no funds moved)");
        }
        AttemptOutcome::NotCleared => {
            return Err(format!(
                "unexpected: UNDEAD->{token} quote didn't clear the near-zero floor — check the pool. \
                 Pivot 1 already {}: you may have an unmatched open position; check {path}.",
                if dry_run { "would have executed" } else { "executed" }
            ));
        }
    }

    println!("New pool {token} <-> UNDEAD bootstrapped. Log: {path}");
    Ok(())
}

//============================================================================
//----- trade / calls: manual and batch execution against calls.csv -------------
//============================================================================
// keystore unlock through swap submission is identical for every
// auto-trading binary, so it's not duplicated here.
//
// trade/calls operate on calls.csv's own from/pivot/proposed shape, which
// isn't necessarily a UNDEAD pool at all — these stay a separate,
// generic ad-hoc mechanism with their own flat log, distinct from the
// per-pool TSV logs new/survey use for the continuous UNDEAD pivot system.
const AD_HOC_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/arbitrage_trades.log");

fn log_trade_outcome(from_symbol: &str, to_symbol: &str, amount: f64, quote_out: f64, tx_hash: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!(
        "{ts},{from_symbol}->{to_symbol},{amount:.6},{quote_out:.8},{tx_hash}\n"
    );
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(AD_HOC_LOG_PATH)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = result {
        eprintln!("Warning: could not write to trade log ({AD_HOC_LOG_PATH}): {e}");
    }
}

async fn run_trade_for_symbols(
    wallet_address: &str,
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
    min_floor: f64,
    slippage_bps: u16,
    dry_run: bool,
    debug: bool,
) -> ErrStr<()> {
    if amount <= 0.0 {
        return Err("amount must be greater than zero".to_string());
    }
    if min_floor <= 0.0 {
        return Err("min_floor must be greater than zero".to_string());
    }

    let available = wallet_balance(wallet_address, from_symbol, registry).await?;
    println!("Wallet ({wallet_address}): {available:.6} {from_symbol} available");
    if available + 1e-6 < amount {
        return Err(format!(
            "Insufficient {from_symbol} — need {amount:.6}, only {available:.6} available. \
             That's not happening. No funds used."
        ));
    }

    let swap = kyber_swap(registry, from_symbol, to_symbol, amount).await?;
    println!("Live swap: {amount:.6} {from_symbol} -> {:.8} {to_symbol} right now", swap.amount_out);
    println!("Your floor: {min_floor:.8} {to_symbol}");

    if swap.amount_out < min_floor {
        return Err(format!(
            "Swap ({:.8} {to_symbol}) is below your floor ({min_floor:.8} {to_symbol}). \
             That's not happening. No funds used.",
            swap.amount_out
        ));
    }

    if dry_run {
        println!(">>> DRY RUN: swap clears your floor. No keystore touched, nothing sent, no funds moved.");
        return Ok(());
    }

    if debug {
        println!(">>> Swap clears your floor. Proceeding to execute.");
    }

    match execute_trade(wallet_address, registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, KEYSTORE_PATH_VAR, debug).await {
        Ok((tx_hash, _gas_avax)) => {
            println!(">>> Trade complete. Tx hash: {tx_hash}");
            log_trade_outcome(from_symbol, to_symbol, amount, swap.amount_out, &tx_hash);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Closes (or opens — direction is whatever the row says, pivot_token ->
/// proposed_token) exactly one row from calls.csv; amount/min_floor are yours to set,
/// independent of whatever calls.csv itself suggests for that row.
pub async fn run_trade(root_url: &str, ix: usize, amount: f64, min_floor: f64, slippage_bps: u16, dry_run: bool, debug: bool) -> ErrStr<()> {
    let wallet_address = wallet_address_from_env("WALLET_ADDRESS")?;
    let registry = load_token_registry()?;

    let calls: Vec<Call> = fetch_calls(root_url)
        .await
        .map_err(|e| format!("Could not fetch calls.csv from {root_url}: {e}"))?;

    let call = calls.iter().find(|c| c.ix == ix)
        .ok_or_else(|| format!("No row with ix {ix} found in calls.csv at {root_url}"))?;

    let from_symbol = call.pivot_token.as_str();
    let to_symbol = call.proposed_token.as_str();

    if token_entry(&registry, from_symbol).is_err() || token_entry(&registry, to_symbol).is_err() {
        return Err(format!("Row {ix}: '{from_symbol}' or '{to_symbol}' not in tokens.toml — refusing to trade. No funds moved."));
    }

    println!("Row {ix}: {from_symbol} -> {to_symbol} (direction fixed by the row)");
    run_trade_for_symbols(&wallet_address, &registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, dry_run, debug).await
}

/// Reads calls.csv and either executes EVERY row or none of them — true
/// go/no-go. Every row is validated against its own gain_10_percent floor
/// first; only if every row clears does any trade actually execute.
///
/// One honest limit: each row is still its own on-chain transaction, not
/// one atomic batch, so this validates all-or-nothing but can't *execute*
/// as a single atomic unit — a later row's price could still move between
/// validation and its own turn to execute. Each trade re-checks its own
/// floor right before it fires (via run_trade_for_symbols), so nothing
/// executes below its floor regardless, but "all N execute" isn't a
/// blockchain-level guarantee, just as close as sequential real swaps get.
pub async fn run_calls_batch(root_url: &str, slippage_bps: u16, dry_run: bool, debug: bool) -> ErrStr<()> {
    let wallet_address = wallet_address_from_env("WALLET_ADDRESS")?;
    let registry = load_token_registry()?;

    let calls: Vec<Call> = fetch_calls(root_url)
        .await
        .map_err(|e| format!("Could not fetch calls.csv from {root_url}: {e}"))?;
    println!("Fetched {} call(s) from {root_url}", calls.len());

    let mut validated = Vec::with_capacity(calls.len());
    for call in &calls {
        let from_symbol = call.pivot_token.as_str();
        let to_symbol = call.proposed_token.as_str();
        let amount = call.pivot_amount as f64;
        let min_floor = call.gain_10_percent as f64;

        if token_entry(&registry, from_symbol).is_err() || token_entry(&registry, to_symbol).is_err() {
            return Err(format!(
                "Call #{}: '{from_symbol}' or '{to_symbol}' not in tokens.toml. This is a \
                 go/no-go batch — one row failing means none execute. No funds moved.",
                call.ix
            ));
        }

        let available = wallet_balance(&wallet_address, from_symbol, &registry).await?;
        if available + 1e-6 < amount {
            return Err(format!(
                "Call #{}: insufficient {from_symbol} — need {amount:.6}, only {available:.6} \
                 available. This is a go/no-go batch — one row failing means none execute. No funds moved.",
                call.ix
            ));
        }

        let swap = kyber_swap(&registry, from_symbol, to_symbol, amount).await?;
        println!("  Call #{}: {amount:.6} {from_symbol} -> {:.8} {to_symbol} swapped (10%-gain floor {min_floor:.8})", call.ix, swap.amount_out);
        if swap.amount_out < min_floor {
            return Err(format!(
                "Call #{}: swap ({:.8} {to_symbol}) is below its 10%-gain floor ({min_floor:.8} \
                 {to_symbol}). This is a go/no-go batch — one row failing means none execute. No funds moved.",
                call.ix, swap.amount_out
            ));
        }

        validated.push((call, from_symbol, to_symbol, amount, min_floor));
    }

    if dry_run {
        println!("All {} call(s) cleared their 10%-gain floor. [DRY RUN] would execute all of them now.", validated.len());
        return Ok(());
    }

    println!("All {} call(s) cleared their 10%-gain floor. Executing.", validated.len());
    for (call, from_symbol, to_symbol, amount, min_floor) in validated {
        println!("--- Call #{} ({from_symbol} -> {to_symbol}) ---", call.ix);
        run_trade_for_symbols(&wallet_address, &registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, false, debug).await?;
    }
    Ok(())
}

//============================================================================
//----- CLI ---------------------------------------------------------------------
//============================================================================
#[derive(Debug, Subcommand)]
enum Command {
    /// Close exactly one row from calls.csv by its 'ix'. Direction fixed by the row
    Trade {
        ix: usize,
        amount: f64,
        min_floor: f64,
        #[arg(long, env = "PIVOT_URL")]
        root_url: String,
        #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
        slippage_bps: u16,
    },
    /// Read calls.csv and execute EVERY row, or none at all — true go/no-go
    Calls {
        #[arg(long, env = "PIVOT_URL")]
        root_url: String,
        #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
        slippage_bps: u16,
    },
    /// Bootstrap a brand-new TOKEN<->UNDEAD pool with a matching pair of opens
    New {
        /// Token symbol — must already be in tokens.toml first
        token: String,
        /// Amount of TOKEN to open the first pivot with
        amount: f64,
        #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
        slippage_bps: u16,
    },
}

#[derive(Debug, Parser)]
#[command(name = "arbitrage")]
#[command(version = "0.16.0")]
struct Args {
    /// No subcommand = full survey: walk every existing pool's log and
    /// close what's ready to close.
    #[command(subcommand)]
    command: Option<Command>,

    /// Applies across every subcommand and the default survey — checks
    /// only, never touches the keystore or sends a tx. `global = true`
    /// means it can go either before or after the subcommand, e.g. both
    /// `arbitrage --dry-run new PAXG 2.0` and `arbitrage new PAXG 2.0 --dry-run` work.
    #[arg(long, global = true, default_value_t = false)]
    dry_run: bool,

    /// Applies across every subcommand and the default survey. Without
    /// it, a trade collapses to one concise line instead of the full
    /// approve/quote/swap play-by-play. Also `global = true` — can go
    /// before or after the subcommand.
    #[arg(long, global = true, default_value_t = false)]
    debug: bool,
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    match args.command {
        None => run_survey(args.dry_run, args.debug).await,
        Some(Command::Trade { ix, amount, min_floor, root_url, slippage_bps }) => {
            run_trade(&root_url, ix, amount, min_floor, slippage_bps, args.dry_run, args.debug).await
        }
        Some(Command::Calls { root_url, slippage_bps }) => {
            run_calls_batch(&root_url, slippage_bps, args.dry_run, args.debug).await
        }
        Some(Command::New { token, amount, slippage_bps }) => {
            run_new(&token, amount, slippage_bps, args.dry_run, args.debug).await
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
    fn test_load_token_registry_has_expected_tokens() -> ErrStr<()> {
        let registry = load_token_registry()?;
        for symbol in ["AVAX", "BTC", "ETH", "USDC", "UNDEAD"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in tokens.toml");
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_run_trade_for_symbols_rejects_zero_or_negative_amounts() -> ErrStr<()> {
        let registry = load_token_registry()?;
        let dummy_wallet = "0x0000000000000000000000000000000000dEaD";
        assert!(run_trade_for_symbols(dummy_wallet, &registry, "BTC", "ETH", 0.0, 1.0, 50, true, false).await.is_err());
        assert!(run_trade_for_symbols(dummy_wallet, &registry, "BTC", "ETH", 1.0, 0.0, 50, true, false).await.is_err());
        assert!(run_trade_for_symbols(dummy_wallet, &registry, "BTC", "ETH", -1.0, 1.0, 50, true, false).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_run_new_rejects_zero_or_negative_amount_before_any_network_call() {
        // amount <= 0.0 is checked before wallet_address_from_env/token_entry,
        // so this never touches the network — mirrors run_trade_for_symbols's
        // own early-return shape.
        assert!(run_new("BTC", 0.0, 50, true, false).await.is_err());
    }

    // biggest_first itself is shared with tvá and tested once in
    // trading::auto_trading's own unit tests — nothing arbitrage-specific
    // left to cover here.
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


    const PIVOT_ROOT_URL: &str = "https://raw.githubusercontent.com/pivoteur/pivoteur.github.io";
    const TEST_WALLET: &str = "0xd16E431b1363Ed90C4fD4906Cf7Fc33E51115429";

    create_testing!("quiz12::arbitrage");

    run!("wallet_balance", " (real ETH read against dedicated test wallet, read-only)", {
        let registry = load_token_registry()?;
        let balance = now(wallet_balance(TEST_WALLET, "ETH", &registry))?;
        println!("\ttest wallet ETH balance: {balance:.4}");
    });

    run!("amount_swapped_eth_to_btc", " (real KyberSwap route, read-only, small ETH->BTC)", {
        let registry = load_token_registry()?;
        let swap = now(kyber_swap(&registry, "ETH", "BTC", 0.01))?;
        println!("\t0.01 ETH -> {:.4} BTC right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("amount_swapped_btc_to_eth", " (real KyberSwap route, read-only, small BTC->ETH)", {
        let registry = load_token_registry()?;
        let swap = now(kyber_swap(&registry, "BTC", "ETH", 0.0001))?;
        println!("\t0.0001 BTC -> {:.4} ETH right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("trade_by_row_dry_run", " (real calls.csv fetch, real row, read-only per-row check)", {
        let registry = load_token_registry()?;
        let calls = now(fetch_calls(PIVOT_ROOT_URL))?;
        if let Some(call) = calls.first() {
            let available = now(wallet_balance(TEST_WALLET, call.pivot_token.as_str(), &registry))?;
            if available <= 0.0 {
                println!("\tskipping: test wallet currently has 0 {} — can't exercise row {}", call.pivot_token, call.ix);
            } else {
                let amount = (call.pivot_amount as f64).min(available * 0.5);
                now(run_trade(PIVOT_ROOT_URL, call.ix, amount, NO_REAL_FLOOR, 50, true, false))?;
                println!("\ttrade dry run completed for row {} without touching the keystore ({amount:.6} {} checked)", call.ix, call.pivot_token);
            }
        } else {
            println!("\tskipping: calls.csv has no rows right now");
        }
    });

    run!("calls_batch_dry_run", " (real calls.csv fetch + read-only per-row checks)", {
        match now(run_calls_batch(PIVOT_ROOT_URL, 50, true, false)) {
            Ok(()) => println!("\tcalls batch dry run completed without touching the keystore"),
            Err(e) if e.to_lowercase().contains("insufficient") => {
                println!("\tskipping strict pass: {e} (reflects the test wallet's real balance right now)");
            }
            Err(e) => return Err(e),
        }
    });

    run!("new_pool_dry_run", " (real quotes for a not-yet-bootstrapped pool, read-only)", {
        let path = pool_log_path("USDC");
        if std::path::Path::new(&path).exists() {
            println!("\tskipping: USDC pool already has a log at {path} — this test only covers a truly fresh pool");
        } else {
            now(run_new("USDC", 1.0, 50, true, false))?;
            println!("\tnew-pool dry run completed for USDC without touching the keystore or writing a log (dry run never creates the file)");
        }
    });

    run!("survey_dry_run", " (dry run across every existing pool log, read-only)", {
        now(run_survey(true, false))?;
        println!("\tsurvey dry run completed without touching the keystore");
    });
}
