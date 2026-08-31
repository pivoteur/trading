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
            wallet_balance,
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

//----- Token Registry --------------------------------------------------------
const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");

pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}
//----- Fixed Trade Sizes ------------------------------------------------------
// tvá trades exactly one pair, BTC<->UNDEAD (unlike arbitrage, which
// surveys many pools). Named as a constant to match UNDEAD below.
const BTC: &str = "BTC";
const DEFAULT_UNDEAD_TRADE_AMOUNT: f64 = 500_000.0;
const DEFAULT_BTC_TRADE_AMOUNT: f64 = 0.005;
const SLIPPAGE_BPS: u16 = 30;
const DEFAULT_DIV_PCT: f64 = 25.0;
//----- Reporting -----------------------------------------------------------
// What tvá's wallet actually started with, for the daily report's "started
// with..." line -- historical, not derived from the log.
const STARTING_UNDEAD_CAPITAL: f64 = 5_000_000.0;
const STARTING_BTC_CAPITAL: f64 = 0.05;
//----- Trade Log — human-readable timestamps -----------------------------------
// Log path is a required runtime argument (--log-path), not a compile-time
// const -- so a real run and a functional test can point at different logs
// without a rebuild. CLI-only, no env fallback -- it's always passed in.

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

//----- Cycle Context & State ---------------------------------------------------
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

//----- One Trading Cycle -------------------------------------------------------
// `wallet_address`/`vault_address` are plain parameters, not resolved from
// env in here -- only `runoff_with_args` (the CLI entrypoint) does that,
// via `resolve_wallet_address`. See the `Args` struct below.
#[allow(clippy::too_many_arguments)]
pub async fn run_cycle(wallet_address: &str, vault_address: &str, keystore_path: &str, log_path: &str, blockchain: &str, btc_trade_amount: f64, undead_trade_amount: f64, pct: f64, dry_run: bool, debug: bool) -> ErrStr<()> {
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
    open_trade(&ctx, &mut state, &mut snap, &mut report, BTC, UNDEAD, btc_trade_amount).await;
    open_trade(&ctx, &mut state, &mut snap, &mut report, UNDEAD, BTC, undead_trade_amount).await;

    assesment_report(&ctx, report.opened_something, state.running_stats, &report.skipped_reasons).await;

    Ok(())
}

/// Daily-cadence status report -- a raw closes/opens tally doesn't say
/// whether the program is doing well, so this reads as a running scoreboard
/// against tvá's actual starting capital instead.
async fn assesment_report(ctx: &CycleCtx<'_>, opened_something: bool, running_stats: CumulativeStats, skipped_open_reasons: &[String]) {
    if !opened_something {
        println!("  Nothing to open this cycle ({}) — see ya in an hour!", skipped_open_reasons.join("; "));
    }

    // total_opens/total_closes are lifetime counts -- their difference is
    // exactly how many pivots are sitting open right now.
    let open_pivots_now = running_stats.total_opens.saturating_sub(running_stats.total_closes);

    let wallet_gas_avax = wallet_balance(ctx.wallet_address, "AVAX", ctx.registry).await
        .unwrap_or_else(|e| {
            eprintln!("  ! WARNING: could not read current AVAX balance for the report ({e}) — showing 0.0.");
            0.0
        });

    println!();
    println!("started with {STARTING_UNDEAD_CAPITAL:.0} UNDEAD and {STARTING_BTC_CAPITAL:.4} BTC, and from pivoting:");
    println!("  open pivots right now:    {open_pivots_now}");
    println!("  pool roi:                 {:.2}%", running_stats.avg_roi() * 100.0);
    println!("  pool apr:                 {:.2}%", running_stats.avg_apr() * 100.0);
    println!("  total profit, UNDEAD:     {:+.8}", running_stats.total_gain_undead);
    println!("  total profit, BTC:        {:+.4}", running_stats.total_gain_asset);
    println!("  total gas used:           {:.5} AVAX", running_stats.total_gas_avax);
    println!("  current gas in wallet:    {wallet_gas_avax:.5} AVAX");

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

//----- div: sending a cut of the surplus to Vault -----------------------------
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

//----- CLI --------------------------------------------------------------------
#[derive(Debug, Parser)]
#[command(name = "tva")]
#[command(version = "1.7.1")]
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
    /// Required, no default, CLI-only (no env fallback) -- a missing log
    /// means missing history, not a fresh start.
    #[arg(long)]
    log_path: String,
    /// Dry-run mode to see what pivots WOULD do, no funds moved or used.
    #[arg(long, global = true, default_value_t = false)]
    dry_run: bool,
    /// debug mode to see what is going on behind the scenes.
    #[arg(short = 'd', global = true, long, default_value_t = false)]
    debug: bool,
    /// Which chain's data/{blockchain}.toml to load.
    #[arg(long, global = true, default_value = "avalanche")]
    blockchain: String,
    /// Amount of BTC to open a new BTC->UNDEAD position with each cycle.
    #[arg(long, global = true, default_value_t = DEFAULT_BTC_TRADE_AMOUNT)]
    btc_trade_amount: f64,
    /// Amount of UNDEAD to open a new UNDEAD->BTC position with each cycle.
    #[arg(long, global = true, default_value_t = DEFAULT_UNDEAD_TRADE_AMOUNT)]
    undead_trade_amount: f64,
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
    run_cycle(&wallet_address, &vault_address, &args.keystore_path, &args.log_path, &args.blockchain, args.btc_trade_amount, args.undead_trade_amount, pct, args.dry_run, args.debug).await
}

//----- UNIT TESTS -------------------------------------------------------------
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

    #[test]
    fn test_trade_amount_flags_default_and_override() {
        let defaults = Args::try_parse_from(["tva", "--keystore-path", "unused", "--log-path", "unused"])
            .expect("should parse with no trade-amount flags given");
        assert_eq!(defaults.btc_trade_amount, DEFAULT_BTC_TRADE_AMOUNT, "with no override, BTC trade size must stay tvá's historical fixed size");
        assert_eq!(defaults.undead_trade_amount, DEFAULT_UNDEAD_TRADE_AMOUNT, "with no override, UNDEAD trade size must stay tvá's historical fixed size");

        let overridden = Args::try_parse_from([
            "tva", "--keystore-path", "unused", "--log-path", "unused",
            "--btc-trade-amount", "0.001", "--undead-trade-amount", "100000",
        ]).expect("should parse with both trade-amount flags given");
        assert_eq!(overridden.btc_trade_amount, 0.001, "--btc-trade-amount must actually override the default");
        assert_eq!(overridden.undead_trade_amount, 100_000.0, "--undead-trade-amount must actually override the default");
    }

    #[test]
    fn test_log_path_is_required_and_cli_only() {
        let result = Args::try_parse_from(["tva", "--keystore-path", "unused"]);
        assert!(result.is_err(), "log_path has no default and no env fallback -- omitting --log-path must fail to parse");
    }

    #[test]
    fn test_amount_decimals_undead_vs_everything_else() {
        assert_eq!(amount_decimals(UNDEAD), 8, "UNDEAD must show 8 decimals");
        assert_eq!(amount_decimals(BTC), 4, "BTC must show 4 decimals");
        assert_eq!(amount_decimals("AVAX"), 4, "anything that isn't UNDEAD must fall back to 4 decimals");
    }

    #[test]
    fn test_compute_div_amount() {
        assert_eq!(compute_div_amount(25.0, 100.0), 25.0, "25% of a 100.0 gain is 25.0");
        assert_eq!(compute_div_amount(0.0, 100.0), 0.0, "0% div sends nothing");
        assert_eq!(compute_div_amount(25.0, -40.0), -10.0, "a negative gain scales through the same way -- callers check amount <= 0.0 themselves");
    }

    #[test]
    fn test_human_ts_formats_a_normal_epoch_without_the_fallback() {
        let formatted = human_ts(0);
        assert!(!formatted.starts_with("(bad timestamp"), "epoch 0 is well within range and must format normally, got: '{formatted}'");
        assert_eq!(formatted.len(), 19, "expected 'YYYY-MM-DD HH:MM:SS' (19 chars), got: '{formatted}'");
    }

    #[test]
    fn test_human_ts_falls_back_on_unparseable_epoch() {
        // 1u64 << 63, reinterpreted as i64, is i64::MIN seconds -- far outside
        // chrono's representable date range, so from_timestamp returns None
        // and human_ts must hit its unwrap_or_else fallback, not panic.
        let formatted = human_ts(1u64 << 63);
        assert!(formatted.starts_with("(bad timestamp:"), "an epoch chrono can't represent must hit the fallback branch, got: '{formatted}'");
    }

    #[test]
    fn test_committed_amt_sums_only_the_matching_proper_token() -> ErrStr<()> {
        let path = std::env::temp_dir().join("tva_test_committed_amt.log");
        let path_str = path.to_str().unwrap();
        std::fs::write(
            &path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n\
             1970-01-01 01:00:00\tOPEN\t2\t\t\tBTC\tUNDEAD\t0.00500000\t495000.00000000\t\t\t\t0.00021000\t0xdef\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        let (opens, ..) = replay_log_with_history_required(path_str)?;
        assert_eq!(opens.len(), 2, "both fixture rows should replay as still-open pivots");

        let btc_committed = committed_amt(BTC, &opens);
        let undead_committed = committed_amt(UNDEAD, &opens);
        assert!((btc_committed - 0.00502601).abs() < 1e-8, "only pivot #1 (proper=BTC) should count toward committed BTC, got {btc_committed}");
        assert!((undead_committed - 495_000.0).abs() < 1e-6, "only pivot #2 (proper=UNDEAD) should count toward committed UNDEAD, got {undead_committed}");

        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}

//----- FUNCTIONAL TESTS -------------------------------------------------------
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use trading::auto_trading::query_swap;

    /// Fixed, hardcoded dummy test addresses -- never read from env.
    const TEST_MANDI_ADDRESS: &str = "0x6700bD7EAE41434f566e48738813fC585B95669a";
    const TEST_SOLONGE_ADDRESS: &str = "0x1111111111111111111111111111111111111E";

    create_testing!("quiz01::a_tva");

    run!("wallet_balance_btc", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            TEST_MANDI_ADDRESS,
            "BTC",
            &registry,
        ))?;
        println!("\ttest wallet BTC balance: {balance:.8}");
    });

    run!("wallet_balance_undead", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            TEST_MANDI_ADDRESS,
            "UNDEAD",
            &registry,
        ))?;
        println!("\ttest wallet UNDEAD balance: {balance:.2}");
    });

    run!("wallet_balance_avax_native_coin_branch", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            TEST_MANDI_ADDRESS,
            "AVAX",
            &registry,
        ))?;
        println!("\ttest wallet AVAX (native) balance: {balance:.5}");
    });

    run!("live_quote_undead_to_btc", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let swap = now(query_swap("avalanche", &registry, "UNDEAD", "BTC", 500_000.0, true))?;
        println!("\t500000 UNDEAD -> {:.8} BTC right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("live_quote_btc_to_undead", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let swap = now(query_swap("avalanche", &registry, "BTC", "UNDEAD", 0.005, true))?;
        println!("\t0.005 BTC -> {:.4} UNDEAD right now (router: {})", swap.amount_out, swap.router_address);
    });

    run!("cycle_dry_run", {
        let log_path = std::env::temp_dir().join("a_tva_functional_test_cycle_dry_run.log");
        let log_path_str = log_path.to_str().unwrap();
        std::fs::write(
            &log_path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        now(run_cycle(TEST_MANDI_ADDRESS, TEST_SOLONGE_ADDRESS, "unused-in-dry-run", log_path_str, "avalanche", DEFAULT_BTC_TRADE_AMOUNT, DEFAULT_UNDEAD_TRADE_AMOUNT, 25.0, true, false))?;
        println!("\tdry-run cycle completed without touching the keystore or any env var");

        let _ = std::fs::remove_file(&log_path);
    });

    run!("cycle_dry_run_custom_trade_amounts", {
        let log_path = std::env::temp_dir().join("a_tva_functional_test_custom_trade_amounts.log");
        let log_path_str = log_path.to_str().unwrap();
        std::fs::write(
            &log_path,
            "1970-01-01 00:16:40\tOPEN\t1\t\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
        ).map_err(|e| format!("could not write test fixture: {e}"))?;

        now(run_cycle(TEST_MANDI_ADDRESS, TEST_SOLONGE_ADDRESS, "unused-in-dry-run", log_path_str, "avalanche", 0.001, 100_000.0, 25.0, true, false))?;
        println!("\tdry-run cycle completed with custom trade amounts (0.001 BTC / 100000 UNDEAD), without touching the keystore");

        let _ = std::fs::remove_file(&log_path);
    });
}
