use std::eprintln;
use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        file_utils::read_file,
        parse_args_add_banner,
};
use libs::types::util::Id;
use trading::auto_trading::{
            TokenRegistry,
            parse_token_registry,
            resolve_wallet_address,
            send_tokens_to_address,
            OpenPivot,
            CumulativeStats,
            BalanceSnapshot,
            balance_snapshot,
            AttemptOutcome,
            attempt_trade_with_actual_amount,
            biggest_first,
            now_ts,
            replay_log,
            log_open,
            log_close,
            log_misfire,
            UNDEAD,
            NO_REAL_FLOOR,
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================
const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");

pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}
//============================================================================
//----- Fixed Trade Sizes ------------------------------------------------------
//============================================================================
// tvá trades exactly one pair, BTC<->UNDEAD (unlike arbitrage, which
// surveys many pools). Named as a constant to match UNDEAD below.
const BTC: &str = "BTC";
const UNDEAD_TRADE_AMOUNT: f64 = 500_000.0;
const BTC_TRADE_AMOUNT: f64 = 0.005;
const SLIPPAGE_BPS: u16 = 30;
const DEFAULT_DIV_PCT: f64 = 25.0;
//============================================================================
//----- Trade Log — human-readable timestamps -----------------------------------
//============================================================================
// Log path is a required runtime argument (--log-path / TVA_LOG_PATH), not
// a compile-time const -- so a real run and a functional test can point at
// different logs without a rebuild.

fn human_ts(epoch: u64) -> String {
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

fn replay_log_with_history_required(path: &str) -> ErrStr<(Vec<OpenPivot>, Id, Id, CumulativeStats)> {
    if !std::path::Path::new(path).exists() {
        return Err(format!(
            "Where is tvá's history? Could not find trade log at '{path}'. \
             This program is wired to real, pre-existing trade history — a missing \
             log file means the path is wrong, not that this is a fresh start. Refusing to run."
        ));
    }
    replay_log(path)
}

//============================================================================
//----- Cycle Context & State ---------------------------------------------------
//============================================================================
// Everything a single cycle needs that stays constant for its whole duration
// (wallet, secrets, chain, registry, the dry-run/debug flags) lives here once,
// instead of being threaded through every helper fn as its own parameter --
// this is what gets `open_trade`/`pivot_survey` under clippy's
// too-many-arguments threshold honestly, instead of just `#[allow(...)]`.
struct CycleCtx<'a> {
    wallet_address: &'a str,
    vault_address: &'a str,
    keystore_path: &'a str,
    log_path: &'a str,
    blockchain: &'a str,
    registry: &'a TokenRegistry,
    dry_run: bool,
    debug: bool,
}

/// The running totals that accumulate across one cycle's survey-then-open
/// passes. Grouped together because they're always updated together and
/// always passed to the same places.
struct CycleState {
    next_pivot_id: Id,
    next_close_id: Id,
    committed_btc: f64,
    committed_undead: f64,
    running_stats: CumulativeStats,
}

/// What the open-pass reports back once both `open_trade` calls are done.
#[derive(Default)]
struct OpenReport {
    opened_something: bool,
    skipped_reasons: Vec<String>,
}

fn committed_amt(token: &str, open_pivots: &[OpenPivot]) -> f64 {
    open_pivots.iter().filter(|p| p.proper == token).map(|p| p.proper_amount).sum()
}

//============================================================================
//----- One Trading Cycle -------------------------------------------------------
//============================================================================
// `wallet_address`/`vault_address` are plain parameters, not resolved from
// env in here -- only `runoff_with_args` (the CLI entrypoint) does that,
// via `resolve_wallet_address`. See the `Args` struct below.
pub async fn run_cycle(wallet_address: &str, vault_address: &str, keystore_path: &str, log_path: &str, blockchain: &str, pct: f64, dry_run: bool, debug: bool) -> ErrStr<()> {
    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry = load_token_registry(&tokens)?;
    let ctx = CycleCtx { wallet_address, vault_address, keystore_path, log_path, blockchain, registry: &registry, dry_run, debug };

    let (open_pivots0, next_pivot_id, next_close_id, opening_stats) = replay_log_with_history_required(log_path)?;
    let open_pivots = biggest_first(open_pivots0);

    let mode_tag = if ctx.dry_run { " [DRY RUN]" } else { "" };
    if ctx.debug {
        println!();
        println!("tvá — {}{mode_tag} — {} open pivot(s), wallet {}", human_ts(now_ts()), open_pivots.len(), ctx.wallet_address);
        println!();
    }

    let mut state = CycleState {
        next_pivot_id,
        next_close_id,
        committed_btc: committed_amt(BTC, &open_pivots),
        committed_undead: committed_amt(UNDEAD, &open_pivots),
        running_stats: opening_stats,
    };

    let mut closed_something = false;

    if ctx.debug {
        eprintln!("surveying {} open pivot(s) this cycle", open_pivots.len());
    }
    for pivot in open_pivots {
        pivot_survey(&ctx, &mut state, &mut closed_something, pivot, pct).await;
    }

    if !closed_something {
        println!("  Nothing to close this cycle — see ya in an hour!");
    }

    let mut snap = balance_snapshot(ctx.wallet_address, ctx.registry, BTC, state.committed_btc, state.committed_undead).await?;
    if ctx.debug {
        println!();
        println!(
            "Wallet's Status = BTC {:.4} in wallet ({:.4} committed, {:.4} available) | UNDEAD {:.8} in wallet ({:.8} committed, {:.8} available)",
            snap.asset_balance, snap.asset_committed, snap.asset_available, snap.undead_balance, snap.undead_committed, snap.undead_available
        );
        println!();
    }

    let mut report = OpenReport::default();
    open_trade(&ctx, &mut state, &mut snap, &mut report, BTC, UNDEAD, BTC_TRADE_AMOUNT).await;
    open_trade(&ctx, &mut state, &mut snap, &mut report, UNDEAD, BTC, UNDEAD_TRADE_AMOUNT).await;

    assesment_report(report.opened_something, state.running_stats, &report.skipped_reasons);

    Ok(())
}

fn assesment_report(opened_something: bool, running_stats: CumulativeStats, skipped_open_reasons: &[String]) {
    if !opened_something {
        println!("  Nothing to open this cycle ({}) — see ya in an hour!", skipped_open_reasons.join("; "));
    }

    println!(
        "[REPORT] Totals = {} closes, {} opens   gain BTC {:+.4}   gain UNDEAD {:+.8}   gas {:.5} AVAX   avg roi {:.2}%   avg apr {:.2}%",
        running_stats.total_closes, running_stats.total_opens,
        running_stats.total_gain_asset, running_stats.total_gain_undead, running_stats.total_gas_avax,
        running_stats.avg_roi() * 100.0, running_stats.avg_apr() * 100.0
    );

    if running_stats.total_gain_asset < 0.0 || running_stats.total_gain_undead < 0.0 {
        println!(
            "  \u{26A0} WARNING: realized cumulative gain is negative — this is NOT a timing artifact, \
             closes are actually losing money. BTC {:+.4}   UNDEAD {:+.8}",
            running_stats.total_gain_asset, running_stats.total_gain_undead
        );
    }
}

/// UNDEAD shows 8 decimals, BTC 4
fn amount_decimals(token: &str) -> usize {
    if token == UNDEAD { 8 } else { 4 }
}

/// Opens one leg (`from` -> `to`) for the given fixed `amount`, checked
/// against the wallet's currently-available balance of `from` off `snap`.
/// Used for both legs tvá opens (BTC->UNDEAD and UNDEAD->BTC); only
/// direction and which committed total gets credited differ. Never
/// propagates an error -- one bad leg can't cancel the other leg or the
/// cycle's summary report.
async fn open_trade(ctx: &CycleCtx<'_>, state: &mut CycleState, snap: &mut BalanceSnapshot, report: &mut OpenReport, from: &str, to: &str, amount: f64) {
    let available = if from == BTC { snap.asset_available } else { snap.undead_available };

    if available <= amount {
        let decimals = amount_decimals(from);
        report.skipped_reasons.push(format!("{from} free balance {available:.decimals$} <= {amount}"));
        return;
    }

    let pivot_id = state.next_pivot_id;
    let from_dp = amount_decimals(from);
    let to_dp = amount_decimals(to);
    match attempt_trade_with_actual_amount(
       ctx.blockchain, ctx.wallet_address, ctx.registry, from, to, amount, NO_REAL_FLOOR, SLIPPAGE_BPS, ctx.keystore_path, ctx.dry_run, ctx.debug,
    ).await {
        Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
            println!("  OPENED  #{pivot_id:<4} {amount:.from_dp$} {from} -> {actual_received:.to_dp$} {to}   gas {gas_avax:.5} AVAX");
            if to == BTC { state.committed_btc += actual_received; } else { state.committed_undead += actual_received; }
            state.running_stats.total_opens += 1;
            state.running_stats.total_gas_avax += gas_avax;

            match balance_snapshot(ctx.wallet_address, ctx.registry, BTC, state.committed_btc, state.committed_undead).await {
                Ok(fresh) => *snap = fresh,
                Err(e) => eprintln!("  ! WARNING: pivot #{pivot_id} opened, but the post-open balance snapshot failed ({e}). Logging the open now anyway with the last-known snapshot -- re-check wallet balances by hand."),
            }
            log_open(ctx.log_path, None, pivot_id, from, amount, to, actual_received, gas_avax, &tx_hash, &*snap, &state.running_stats);
            report.opened_something = true;
        }
        Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
            println!("  WOULD OPEN  #{pivot_id:<4} {amount:.from_dp$} {from} -> ~{quoted_amount_out:.to_dp$} {to}");
            report.opened_something = true;
        }
        Ok(AttemptOutcome::NotCleared) => println!("  ! unexpected: #{pivot_id:<4} {from}->{to} open quote didn't clear the near-zero floor — check the pool."),
        Err(e) => {
            println!("  ! open ({from}->{to}) failed: {e}");
            if !ctx.dry_run {
                log_misfire(ctx.log_path, None, from, to, amount, 0.0, "", &*snap, &state.running_stats);
            }
        }
    }
    state.next_pivot_id += 1;
}

/// Surveys one pivot for a possible close. Never propagates an error, same
/// as `open_trade` -- one bad pivot can't cancel the rest of the survey or
/// the open-new-position step that follows it.
async fn pivot_survey(ctx: &CycleCtx<'_>, state: &mut CycleState, closed_something: &mut bool, pivot: OpenPivot, pct: f64) {
    let prim_dp = amount_decimals(&pivot.prim);
    let proper_dp = amount_decimals(&pivot.proper);

    match attempt_trade_with_actual_amount(
        ctx.blockchain, ctx.wallet_address, ctx.registry, &pivot.proper, &pivot.prim,
        pivot.proper_amount, pivot.prim_amount, SLIPPAGE_BPS, ctx.keystore_path, ctx.dry_run, ctx.debug,
    ).await {
        Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
            if ctx.dry_run { panic!("dry run should never return Executed — that would mean funds were actually moved!"); }
            let gain = actual_received - pivot.prim_amount;
            let roi = gain / pivot.prim_amount;
            let days_held = (now_ts().saturating_sub(pivot.opened_at)) as f64 / 86_400.0;
            let apr = if days_held > 0.0 { roi * 365.0 / days_held } else { 0.0 };

            if pivot.proper == BTC {
                state.committed_btc -= pivot.proper_amount;
            } else if pivot.proper == UNDEAD {
                state.committed_undead -= pivot.proper_amount;
            }
            state.running_stats.total_closes += 1;
            state.running_stats.total_gas_avax += gas_avax;
            state.running_stats.roi_sum += roi;
            state.running_stats.apr_sum += apr;
            if pivot.prim == UNDEAD {
                state.running_stats.total_gain_undead += gain;
            } else {
                state.running_stats.total_gain_asset += gain;
            }

            println!(
                "  CLOSED  #{:<4}   opened {:.prim_dp$} {} -> {:.proper_dp$} {}   closed -> {:.prim_dp$} {}   gain {:+.prim_dp$} {}   roi {:.2}%   apr {:.2}%   gas {:.5} AVAX",
                pivot.pivot_id, pivot.prim_amount, pivot.prim, pivot.proper_amount, pivot.proper,
                actual_received, pivot.prim, gain, pivot.prim, roi * 100.0, apr * 100.0, gas_avax
            );

            let snap = balance_snapshot(ctx.wallet_address, ctx.registry, BTC, state.committed_btc, state.committed_undead).await
                .unwrap_or_else(|e| {
                    eprintln!("  ! WARNING: pivot #{} closed, but the post-close balance snapshot failed ({e}). Logging the close now anyway (with a zeroed snapshot) so it isn't lost -- re-check wallet balances by hand.", pivot.pivot_id);
                    BalanceSnapshot {
                        asset_balance: 0.0, asset_committed: state.committed_btc, asset_available: 0.0,
                        undead_balance: 0.0, undead_committed: state.committed_undead, undead_available: 0.0,
                    }
                });
            log_close(ctx.log_path, None, pivot.pivot_id, state.next_close_id, &pivot.prim, pivot.prim_amount, &pivot.proper, actual_received, gain, roi, apr, gas_avax, &tx_hash, &snap, &state.running_stats);
            state.next_close_id += 1;
            *closed_something = true;

            // The Vault div is secondary to an already-logged close -- its
            // failure must not hide that the close happened.
            if let Err(e) = divvy_to_vault(ctx, &pivot.prim, gain, pct).await {
                println!("  ! pivot #{} closed and logged, but sending the div to Vault failed: {e} -- needs a manual look.", pivot.pivot_id);
            }
        }
        Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
            if !ctx.dry_run { panic!("not dry run should never return DryRunWouldClear — that would mean funds were actually moved!"); }
            let gain = quoted_amount_out - pivot.prim_amount;
            let roi = gain / pivot.prim_amount;
            println!(
                "  WOULD CLOSE  #{:<4}   opened {:.prim_dp$} {} -> {:.proper_dp$} {}   est. close -> {:.prim_dp$} {}   est. gain {gain:+.prim_dp$}   est. roi {:.2}%",
                pivot.pivot_id, pivot.prim_amount, pivot.prim, pivot.proper_amount, pivot.proper,
                quoted_amount_out, pivot.prim, roi * 100.0
            );
            if let Err(e) = divvy_to_vault(ctx, &pivot.prim, gain, pct).await {
                println!("  ! pivot #{} dry-run div preview failed: {e}", pivot.pivot_id);
            }
            *closed_something = true;
        }
        Ok(AttemptOutcome::NotCleared) => {
            println!(
                "  not yet   #{:<4}   opened {:.prim_dp$} {} -> {:.proper_dp$} {}   now closing back to {}, needs >= {:.prim_dp$} {}",
                pivot.pivot_id, pivot.prim_amount, pivot.prim, pivot.proper_amount, pivot.proper,
                pivot.prim, pivot.prim_amount, pivot.prim
            );
        }
        Err(e) => {
            println!("  ! close attempt for pivot #{} failed, staying open: {e}", pivot.pivot_id);
            if !ctx.dry_run {
                let snap = balance_snapshot(ctx.wallet_address, ctx.registry, BTC, state.committed_btc, state.committed_undead).await
                    .unwrap_or_else(|snap_err| {
                        eprintln!("  ! WARNING: pivot #{} misfire, and the balance snapshot for logging it also failed ({snap_err}). Logging the misfire now anyway (with a zeroed snapshot) so it isn't lost.", pivot.pivot_id);
                        BalanceSnapshot {
                            asset_balance: 0.0, asset_committed: state.committed_btc, asset_available: 0.0,
                            undead_balance: 0.0, undead_committed: state.committed_undead, undead_available: 0.0,
                        }
                    });
                log_misfire(ctx.log_path, None, &pivot.prim, &pivot.proper, pivot.prim_amount, 0.0, "", &snap, &state.running_stats);
            }
        }
    }
}

//============================================================================
//----- div: sending a cut of the surplus to Vault -----------------------------
//============================================================================
fn compute_div_amount(pct: f64, gain: f64) -> f64 { pct / 100.0 * gain }

// `ctx.dry_run` doubles as this fn's own dry-run flag -- `pivot_survey` only
// ever calls this from the `Executed` arm (dry_run proven false) or the
// `DryRunWouldClear` arm (dry_run proven true), so a separate bool here
// would just be a redundant copy of a value the caller already has.
async fn divvy_to_vault(ctx: &CycleCtx<'_>, token: &str, gain: f64, pct: f64) -> ErrStr<()> {
    let mode_tag = if ctx.dry_run { " [DRY RUN]" } else { "" };
    let amount = compute_div_amount(pct, gain);

    println!("  div{mode_tag}: {pct:.2}% of sendable surplus -> Vault ({})", ctx.vault_address);

    if amount <= 0.0 {
        println!("  Nothing sendable right now (no surplus, or it's all committed to open pivots) — no transfer made.");
    }
    else if ctx.dry_run {
        println!("  [DRY-RUN] Would send {amount:.8} {token} to Vault.");
    }
    else {
        let (tx_hash, gas_avax) = send_tokens_to_address(ctx.wallet_address, ctx.registry, token, ctx.vault_address, amount, ctx.keystore_path, ctx.debug).await?;
        println!("  Sent {amount:.8} {token} to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    }
    Ok(())
}

//============================================================================
//----- CLI --------------------------------------------------------------------
//============================================================================
#[derive(Debug, Parser)]
#[command(name = "tva")]
#[command(version = "1.5.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// CLI override; falls back to TVA_WALLET_ADDRESS via `resolve_wallet_address`.
    #[arg(long)]
    wallet_address: Option<String>,
    /// CLI override; falls back to VAULT_ADDRESS via `resolve_wallet_address`.
    #[arg(long)]
    vault_address: Option<String>,
    /// `global = true` lets this be given either before or after the
    /// subcommand — `tva --dry-run div` and `tva div --dry-run` both work.
    #[arg(long, env = "TVA_KEYSTORE_PATH")]
    keystore_path: String,
    /// Required, no default -- a missing log means missing history, not a fresh start.
    #[arg(long, env = "TVA_LOG_PATH")]
    log_path: String,
    #[arg(long, global = true, default_value_t = false)]
    dry_run: bool,
    /// debug mode (e.g. --debug or -d)
    #[arg(short = 'd', global = true, long, default_value_t = false)]
    debug: bool,
    /// Which chain's data/{blockchain}.toml to load. (e.g. 'avalanche')
    #[arg(long, global = true, default_value = "avalanche")]
    blockchain: String,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// divvy a certain percentage to a vault/ separate wallet (e.g. div --pct 25)
    Div {
        #[arg(long, default_value_t = DEFAULT_DIV_PCT)]
        pct: f64,
    },
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let pct = match args.command {
        None => 0.0,
        Some(Command::Div { pct }) => pct,
    };
    let wallet_address = resolve_wallet_address(args.wallet_address, "TVA_WALLET_ADDRESS")?;
    let vault_address = resolve_wallet_address(args.vault_address, "VAULT_ADDRESS")?;
    run_cycle(&wallet_address, &vault_address, &args.keystore_path, &args.log_path, &args.blockchain, pct, args.dry_run, args.debug).await
}

//============================================================================
//----- UNIT TESTS -------------------------------------------------------------
//============================================================================
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_load_token_registry_has_btc_undead_avax() -> ErrStr<()> {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        for symbol in ["BTC", "UNDEAD", "AVAX"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in avalanche.toml");
        }
        Ok(())
    }

    // refuse to run at all when the log is missing,
    // instead of treating it as a fresh start.
    #[test]
    fn test_replay_log_missing_file_is_a_hard_error_not_a_fresh_start() {
        let result = replay_log_with_history_required("/tmp/definitely_does_not_exist.log");
        assert!(result.is_err(), "a missing log file must never be treated as a fresh start — tvá's history is real and pre-existing");
        let msg = result.unwrap_err();
        assert!(msg.contains("history"), "error should make clear this is about missing history, not an empty-is-fine case: '{msg}'");
    }

    #[test]
    fn test_replay_log_with_history_required_delegates_to_shared_parser_when_file_exists() -> ErrStr<()> {
        let path = std::env::temp_dir().join("tva_test_delegates.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, next_pivot, _, stats) = replay_log_with_history_required(path_str)?;
        assert_eq!(opens.len(), 1, "should see the one still-open pivot from the shared parser");
        assert_eq!(next_pivot, 2);
        assert_eq!(stats.total_opens, 1);

        let _ = std::fs::remove_file(&path);
        Ok(())
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
    use trading::auto_trading::{ wallet_balance, kyber_swap };

    /// Fixed, hardcoded dummy test addresses -- never read from env
    const MANDI_ADDRESS: &str = "0x6700bD7EAE41434f566e48738813fC585B59669a";
    const SOLONGE_ADDRESS: &str = "0x1111111111111111111111111111111111111E";

    create_testing!("quiz01::a_tva");

    run!("wallet_balance_btc", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            MANDI_ADDRESS,
            "BTC",
            &registry,
        ))?;
        println!("\ttest wallet BTC balance: {balance:.8}");
    });

    run!("wallet_balance_undead", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            MANDI_ADDRESS,
            "UNDEAD",
            &registry,
        ))?;
        println!("\ttest wallet UNDEAD balance: {balance:.2}");
    });

    run!("live_quote_undead_to_btc", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let swap = now(kyber_swap("avalanche", &registry, "UNDEAD", "BTC", 500_000.0, true))?;
        println!("\t500000 UNDEAD -> {:.8} BTC right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("live_quote_btc_to_undead", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let swap = now(kyber_swap("avalanche", &registry, "BTC", "UNDEAD", 0.005, true))?;
        println!("\t0.005 BTC -> {:.4} UNDEAD right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("cycle_dry_run", {
        let log_path = std::env::temp_dir().join("a_tva_functional_test_cycle_dry_run.log");
        let log_path_str = log_path.to_str().unwrap();
        std::fs::write(
            &log_path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        now(run_cycle(MANDI_ADDRESS, SOLONGE_ADDRESS, "unused-in-dry-run", log_path_str, "avalanche", 25.0, true, false))?;
        println!("\tdry-run cycle completed without touching the keystore or any env var");

        let _ = std::fs::remove_file(&log_path);
    });
}
