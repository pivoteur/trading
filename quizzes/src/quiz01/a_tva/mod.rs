use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        parse_args_add_banner,
};
use trading::auto_trading::{
            TokenRegistry,
            parse_token_registry,
            wallet_address_from_env,
            send_tokens,
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
            UNDEAD,
            NO_REAL_FLOOR,
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
// tvá is wired to exactly one pair — BTC<->UNDEAD — unlike arbitrage,
// which works a whole survey of asset<->UNDEAD pools. BTC is named here
// (rather than kept as a bare literal) purely so it reads consistently
// alongside the shared UNDEAD constant below.
const BTC: &str = "BTC";
/// Env var holding the path to tvá's own encrypted keystore file — kept
/// as one named constant rather than repeating the literal at each call
/// site.
const KEYSTORE_PATH_VAR: &str = "TVA_KEYSTORE_PATH";

const UNDEAD_TRADE_AMOUNT: f64 = 500_000.0;
const BTC_TRADE_AMOUNT: f64 = 0.005;
const SLIPPAGE_BPS: u16 = 200;
const STARTING_UNDEAD: f64 = 5_000_000.0;
const STARTING_BTC: f64 = 0.05;
const ILLUSTRATIVE_SKIM_PCT: f64 = 0.25;
const VAULT_ADDRESS: &str = "VAULT_ADDRESS";
const DEFAULT_DIV_PCT: f64 = 25.0;
//============================================================================
//----- Trade Log — path & human-readable timestamps ---------------------------
//============================================================================
const TRADE_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/tva-trades.log");

fn human_ts(epoch: u64) -> String {
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

fn replay_log_with_history_required(path: &str) -> ErrStr<(Vec<OpenPivot>, u32, u32, CumulativeStats)> {
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
//----- One Trading Cycle -------------------------------------------------------
//============================================================================
pub async fn run_cycle(pct: f64, dry_run: bool, debug: bool) -> ErrStr<()> {
    let wallet_address = wallet_address_from_env("TVA_WALLET_ADDRESS")?;
    let registry = load_token_registry()?; //calling this when calling div
    let (open_pivots0, mut next_pivot_id, mut next_close_id, opening_stats) = replay_log_with_history_required(TRADE_LOG_PATH)?;
    let open_pivots = biggest_first(open_pivots0);

    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };
    if debug {
        println!();
        println!("tvá — {}{mode_tag} — {} open pivot(s), wallet {wallet_address}", human_ts(now_ts()), open_pivots.len());
        println!();
    }

    fn committed_amt(token: &str, open_pivots: &Vec<OpenPivot>) -> f64 {
        open_pivots.iter().filter(|p| p.proper == token).map(|p| p.proper_amount).sum()
    }

    let mut committed_btc: f64 = committed_amt(BTC, &open_pivots);
    let mut committed_undead: f64 = committed_amt(UNDEAD, &open_pivots);
    let mut running_stats = opening_stats;

    let mut closed_something = false;

    for pivot in open_pivots {
        pivot_survey(dry_run, debug, &wallet_address, &registry, &mut next_close_id, &mut committed_btc, &mut committed_undead, &mut running_stats, &mut closed_something, pivot, pct).await?;
    }

    if !closed_something {
        println!("  Nothing to close this cycle — see ya in an hour!");
    }

    let mut snap = balance_snapshot(&wallet_address, &registry, BTC, committed_btc, committed_undead).await?;
    if debug {
        println!();
        println!(
            "Wallet's Status = BTC {:.6} in wallet ({:.6} committed, {:.6} available) | UNDEAD {:.2} in wallet ({:.2} committed, {:.2} available)",
            snap.asset_balance, snap.asset_committed, snap.asset_available, snap.undead_balance, snap.undead_committed, snap.undead_available
        );
        println!();
    }

    let mut opened_something = false;
    let mut skipped_open_reasons: Vec<String> = Vec::new();

    btc_trade(dry_run, debug, &wallet_address, &registry, &mut next_pivot_id, committed_btc, &mut committed_undead, &mut running_stats, &mut snap, &mut opened_something, &mut skipped_open_reasons).await?;
    undead_trade(dry_run, debug, wallet_address, registry, next_pivot_id, committed_btc, committed_undead, &mut running_stats, &mut snap, &mut opened_something, &mut skipped_open_reasons).await?;

    assesment_report(debug, snap, opened_something, running_stats, &skipped_open_reasons);
    
    Ok(())
}

fn assesment_report(debug: bool, snap: BalanceSnapshot, opened_something: bool, running_stats: CumulativeStats, skipped_open_reasons: &[String]) {
    let btc_vs_start = snap.asset_balance - STARTING_BTC;
    let undead_vs_start = snap.undead_balance - STARTING_UNDEAD;
    if !opened_something {
        println!("  Nothing to open this cycle ({}) — see ya in an hour!", skipped_open_reasons.join("; "));
    }
    
    println!(
        "Totals = {} closes, {} opens   gain BTC {:+.8}   gain UNDEAD {:+.4}   gas {:.5} AVAX   avg roi {:.2}%   avg apr {:.2}%",
        running_stats.total_closes, running_stats.total_opens,
        running_stats.total_gain_asset, running_stats.total_gain_undead, running_stats.total_gas_avax,
        running_stats.avg_roi() * 100.0, running_stats.avg_apr() * 100.0
    );
    
    if running_stats.total_gain_asset < 0.0 || running_stats.total_gain_undead < 0.0 {
        println!(
            "  \u{26A0} WARNING: realized cumulative gain is negative — this is NOT a timing artifact, \
             closes are actually losing money. BTC {:+.8}   UNDEAD {:+.4}",
            running_stats.total_gain_asset, running_stats.total_gain_undead
        );
    }
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
}

async fn undead_trade(dry_run: bool, debug: bool, wallet_address: String, registry: std::collections::HashMap<String, trading::auto_trading::TokenEntry>, next_pivot_id: u32, mut committed_btc: f64, committed_undead: f64, running_stats: &mut CumulativeStats, snap: &mut BalanceSnapshot, opened_something: &mut bool, skipped_open_reasons: &mut Vec<String>) -> Result<(), String> {
    Ok(if snap.undead_available > UNDEAD_TRADE_AMOUNT {
        match attempt_trade_with_actual_amount(
            &wallet_address, &registry, UNDEAD, BTC, UNDEAD_TRADE_AMOUNT, NO_REAL_FLOOR, SLIPPAGE_BPS, KEYSTORE_PATH_VAR, dry_run, debug,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> {actual_received:.8} BTC   gas {gas_avax:.5} AVAX");
                committed_btc += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                *snap = balance_snapshot(&wallet_address, &registry, BTC, committed_btc, committed_undead).await?;
                log_open(TRADE_LOG_PATH, None, next_pivot_id, UNDEAD, UNDEAD_TRADE_AMOUNT, BTC, actual_received, gas_avax, &tx_hash, &*snap, &*running_stats);
                *opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{next_pivot_id:<4} {UNDEAD_TRADE_AMOUNT:.2} UNDEAD -> ~{quoted_amount_out:.8} BTC   (dry run, no funds moved)");
                *opened_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => println!("  ! unexpected: UNDEAD->BTC open quote didn't clear the near-zero floor — check the pool."),
            Err(e) => println!("  ! open (UNDEAD->BTC) failed: {e}"),
        }
    } else {
        skipped_open_reasons.push(format!("UNDEAD free balance {:.2} <= {UNDEAD_TRADE_AMOUNT}", snap.undead_available));
    })
}

async fn btc_trade(dry_run: bool, debug: bool, wallet_address: &String, registry: &std::collections::HashMap<String, trading::auto_trading::TokenEntry>, next_pivot_id: &mut u32, committed_btc: f64, committed_undead: &mut f64, running_stats: &mut CumulativeStats, snap: &mut BalanceSnapshot, opened_something: &mut bool, skipped_open_reasons: &mut Vec<String>) -> Result<(), String> {
    Ok(if snap.asset_available > BTC_TRADE_AMOUNT {
        let attempted_id = *next_pivot_id;
        match attempt_trade_with_actual_amount(
            wallet_address, registry, BTC, UNDEAD, BTC_TRADE_AMOUNT, NO_REAL_FLOOR, SLIPPAGE_BPS, KEYSTORE_PATH_VAR, dry_run, debug,
        ).await {
            Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
                println!("  OPENED  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.8} BTC -> {actual_received:.2} UNDEAD   gas {gas_avax:.5} AVAX");
                *committed_undead += actual_received;
                running_stats.total_opens += 1;
                running_stats.total_gas_avax += gas_avax;
                *snap = balance_snapshot(wallet_address, registry, BTC, committed_btc, *committed_undead).await?;
                log_open(TRADE_LOG_PATH, None, attempted_id, BTC, BTC_TRADE_AMOUNT, UNDEAD, actual_received, gas_avax, &tx_hash, &*snap, &*running_stats);
                *opened_something = true;
            }
            Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
                println!("  WOULD OPEN  #{attempted_id:<4} {BTC_TRADE_AMOUNT:.8} BTC -> ~{quoted_amount_out:.2} UNDEAD   (dry run, no funds moved)");
                *opened_something = true;
            }
            Ok(AttemptOutcome::NotCleared) => println!("  ! unexpected: BTC->UNDEAD open quote didn't clear the near-zero floor — check the pool."),
            Err(e) => println!("  ! open (BTC->UNDEAD) failed: {e}"),
        }
        *next_pivot_id += 1;
    } else {
        skipped_open_reasons.push(format!("BTC free balance {:.6} <= {BTC_TRADE_AMOUNT}", snap.asset_available));
    })
}

async fn pivot_survey(dry_run: bool, debug: bool, wallet_address: &String, registry: &std::collections::HashMap<String, trading::auto_trading::TokenEntry>, next_close_id: &mut u32, committed_btc: &mut f64, committed_undead: &mut f64, running_stats: &mut CumulativeStats, closed_something: &mut bool, pivot: OpenPivot, pct: f64) -> Result<(), String> {
    Ok(match attempt_trade_with_actual_amount(
        wallet_address, registry, &pivot.proper, &pivot.prim,
        pivot.proper_amount, pivot.prim_amount, SLIPPAGE_BPS, KEYSTORE_PATH_VAR, dry_run, debug,
    ).await {
        Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
            let gain = actual_received - pivot.prim_amount;
            divvy_to_vault(wallet_address, registry, &pivot.prim, gain, pct, dry_run, debug).await?;
            let roi = gain / pivot.prim_amount;
            let days_held = (now_ts().saturating_sub(pivot.opened_at)) as f64 / 86_400.0;
            let apr = if days_held > 0.0 { roi * 365.0 / days_held } else { 0.0 };
            println!(
                "  CLOSED  #{:<4} {:.4} {} -> {:.4} {}   gain {:+.4} {}   roi {:.2}%   apr {:.1}%   gas {:.5} AVAX",
                pivot.pivot_id, pivot.proper_amount, pivot.proper, actual_received, pivot.prim,
                gain, pivot.prim, roi * 100.0, apr * 100.0, gas_avax
            );

            if pivot.proper == BTC {
                *committed_btc -= pivot.proper_amount;
            } else if pivot.proper == UNDEAD {
                *committed_undead -= pivot.proper_amount;
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

            let snap = balance_snapshot(wallet_address, registry, BTC, *committed_btc, *committed_undead).await?;
            log_close(TRADE_LOG_PATH, None, pivot.pivot_id, *next_close_id, &pivot.prim, pivot.prim_amount, &pivot.proper, actual_received, gain, roi, apr, gas_avax, &tx_hash, &snap, &*running_stats);
            *next_close_id += 1;
            *closed_something = true;
        }
        Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
            let gain = quoted_amount_out - pivot.prim_amount;
            let roi = gain / pivot.prim_amount;
            println!(
                "  WOULD CLOSE  #{:<4} {:.8} {} -> ~{:.4} {}   est. gain {:+.4} {}   est. roi {:.2}%   (dry run, no funds moved)",
                pivot.pivot_id, pivot.proper_amount, pivot.proper, quoted_amount_out, pivot.prim,
                gain, pivot.prim, roi * 100.0
            );
            *closed_something = true;
        }
        Ok(AttemptOutcome::NotCleared) => {}
        Err(e) => {
            println!("  ! close attempt for pivot #{} failed, staying open: {e}", pivot.pivot_id);
        }
    })
}

//============================================================================
//----- div: sending a cut of the surplus to Vault -----------------------------
//============================================================================
fn compute_div_amount(pct: f64, gain: f64) -> f64 { pct / 100.0 * gain }

async fn divvy_to_vault(wallet_address: &str, registry: &TokenRegistry, token: &str, gain: f64, pct: f64, dry_run: bool, debug: bool) -> ErrStr<()> {
    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };
    let amount = compute_div_amount(pct, gain);

    println!("  div{mode_tag}: {pct:.1}% of sendable surplus -> Vault ({VAULT_ADDRESS})");
    println!("    {token}   sending {amount:.8}");

    if amount <= 0.0 {
        println!("  Nothing sendable right now (no surplus, or it's all committed to open pivots) — no transfer made.");
        return Ok(());
    }

    if dry_run {
        if amount > 0.0 {
            println!("  Would send {amount:.8} {token} to Vault. (dry run, no funds moved)");
        }
        return Ok(());
    }

    async fn token_to_send(wallet_address: &str, registry: &TokenRegistry, token: &str, vault_address: &str, amount: f64, keystore_var: &str, debug: bool) -> ErrStr<(String, f64)> {
        send_tokens(wallet_address, registry, token, vault_address, amount, keystore_var, debug).await
    }
    
    let (tx_hash, gas_avax) = token_to_send(wallet_address, registry, token, VAULT_ADDRESS, amount, KEYSTORE_PATH_VAR, debug).await?;
    println!("  Sent {amount:.4} {token} to Vault. tx: {tx_hash}   gas {gas_avax:.5} AVAX");
    Ok(())
}

//============================================================================
//----- CLI --------------------------------------------------------------------
//============================================================================
#[derive(Debug, Parser)]
#[command(name = "tva")]
#[command(bin_name = "tva")]
#[command(version = "0.15.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// `global = true` lets this be given either before or after the
    /// subcommand — `tva --dry-run div` and `tva div --dry-run` both work.
    #[arg(long, global = true, default_value_t = false)]
    dry_run: bool,

    #[arg(long, global = true, default_value_t = false)]
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
    let pct = match args.command {
        None => {
            0.0
        }
        Some(Command::Div { pct }) => { pct }
    };
    run_cycle(pct, args.dry_run, args.debug).await

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

    // The pivot-log parsing itself (OPEN/CLOSE replay, malformed lines,
    // orphan CLOSEs, CHECK-row tolerance, sort order, timestamp
    // round-tripping) is shared with arbitrage and tested once in
    // trading::auto_trading's own unit tests. What's tvá-specific — and
    // what belongs here — is this wrapper's one added behavior: refusing
    // to run at all when the log is missing, instead of treating that as
    // a fresh start the way arbitrage's per-pool logs do.
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
            "1970-01-01 00:16:40\tOPEN\t1\t\tUNDEAD\tBTC\t500000.00000000\t0.00502601\t\t\t\t0.00500000\t0xabc\n",
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
    use trading::auto_trading::{ wallet_balance, live_quote };

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
        now(run_cycle(25.0, true, false))?;
        println!("\tdry-run cycle completed without touching the keystore");
    });
}
