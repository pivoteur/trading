use clap::Parser;
use book::{
        cli_utils::generate_banner,
        debug,
        err_utils::ErrStr,
        parse_args_add_banner,
};
use trading::auto_trading::{
            TokenRegistry,
            parse_token_registry,
            wallet_balance,
            kyber_swap,
            attempt_trade_with_actual_amount,
            AttemptOutcome,
            NO_REAL_FLOOR,
            UNDEAD,
            now_ts,
            log_ts,
            append_trade_log_line,
};

const TOKENS_TOML: &str = include_str!("tokens.toml");

pub fn load_token_registry() -> ErrStr<TokenRegistry> {
    parse_token_registry(TOKENS_TOML)
}

// env var name the shared signing core (trading::auto_trading::load_signer)
// looks up internally -- reusing tva's own name defaults this to the same
// wallet/keystore/secrets as tva, no new secrets to provision. --wallet-
// address/--keystore-path below can still override at the CLI to point at
// any other wallet, as long as its tokens are in tokens.toml.
const KEYSTORE_PATH_VAR: &str = "TVA_KEYSTORE_PATH";

const DEFAULT_SLIPPAGE_BPS: u16 = 50;
const DUST_EPSILON: f64 = 1e-8;

// resolves to quizzes/data/, same as tva/arbitrage
const TRADE_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/equalize-undead-btc.log");
const TRADE_LOG_HEADER: &str = "timestamp\tmode\ttoken\tundead_balance\ttoken_balance\ttoken_value_in_undead\tswap_undead\toutcome\tactual_received\tgas_avax\ttx_hash";

#[allow(clippy::too_many_arguments)]
fn log_run(
    mode: &str, token: &str, undead_balance: f64, token_balance: f64,
    token_value_in_undead: f64, swap_undead: f64, outcome: &str,
    actual_received: f64, gas_avax: f64, tx_hash: &str,
) {
    let line = format!(
        "{}\t{mode}\t{token}\t{undead_balance:.8}\t{token_balance:.8}\t{token_value_in_undead:.8}\t{swap_undead:.8}\t{outcome}\t{actual_received:.8}\t{gas_avax:.8}\t{tx_hash}",
        log_ts(now_ts())
    );
    append_trade_log_line(TRADE_LOG_PATH, &line, Some(TRADE_LOG_HEADER));
}

/// Balances UNDEAD against `token` by swapping UNDEAD -> `token` only.
/// No code path here ever calls `token -> UNDEAD`.
#[allow(clippy::too_many_arguments)]
pub async fn run_cycle(token: &str, vault_address: &str, keystore_path: &str, slippage_bps: u16, dry_run: bool, debug: bool) -> ErrStr<()> {
    debug!("run_cycle", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    log!("mode {} token {}", mode, token);

    // bridge the resolved keystore path (CLI override or the TVA_KEYSTORE_PATH
    // default) into the env var name the shared signing core reads itself.
    // SAFETY: single-threaded at this point in main -- no concurrent env reads.
    unsafe { std::env::set_var(KEYSTORE_PATH_VAR, keystore_path); }

    let registry = load_token_registry()?;

    let (undead_balance, token_balance) = tokio::try_join!(
        wallet_balance(vault_address, UNDEAD, &registry),
        wallet_balance(vault_address, token, &registry),
    )?;
    log!("wallet {}", vault_address);
    log!("UNDEAD balance {:.4}", undead_balance);
    log!("{} balance {:.8}", token, token_balance);

    // swap_amount can never exceed undead_balance / 2 (token_value is never
    // negative) -- so that's the reference amount to quote for the rate,
    // never the full balance. This quote size is always a real possible
    // trade size, never a "sell everything" scenario that will never happen.
    let reference_amount = undead_balance / 2.0;
    if reference_amount <= DUST_EPSILON {
        println!("  no UNDEAD to work with -- nothing to swap.");
        log_run(mode, token, undead_balance, token_balance, 0.0, 0.0, "SKIPPED_NO_UNDEAD", 0.0, 0.0, "");
        return Ok(());
    }

    let reference_quote = kyber_swap(blockchain, &registry, UNDEAD, token, reference_amount).await?.amount_out;
    log!("{:.8} UNDEAD (half balance) quotes to {:.8} {}", reference_amount, reference_quote, token);
    if reference_quote <= DUST_EPSILON {
        return Err(format!("KyberSwap quoted ~0 {token} for {reference_amount:.8} UNDEAD -- no route/liquidity right now."));
    }

    let token_value = token_value_in_undead(token_balance, reference_amount, reference_quote);
    log!("{} is worth {:.4} UNDEAD right now", token, token_value);

    let swap_amount = compute_swap_amount(undead_balance, token_value);

    if swap_amount <= DUST_EPSILON {
        println!(
            "  {token} already at/ahead of parity ({token_value:.4} UNDEAD-equiv vs {undead_balance:.4} UNDEAD held) -- no swap."
        );
        log_run(mode, token, undead_balance, token_balance, token_value, swap_amount.max(0.0), "SKIPPED_ALREADY_BALANCED", 0.0, 0.0, "");
        return Ok(());
    }

    println!("  swapping {swap_amount:.8} UNDEAD -> {token}");

    match attempt_trade_with_actual_amount(
        vault_address, &registry, UNDEAD, token, swap_amount, NO_REAL_FLOOR, slippage_bps, KEYSTORE_PATH_VAR, dry_run, debug,
    ).await {
        Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
            println!("  SWAPPED  {swap_amount:.8} UNDEAD -> {actual_received:.8} {token}   gas {gas_avax:.5} AVAX   tx {tx_hash}");
            log_run(mode, token, undead_balance, token_balance, token_value, swap_amount, "SWAPPED", actual_received, gas_avax, &tx_hash);
        }
        Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
            println!("  WOULD SWAP  {swap_amount:.8} UNDEAD -> ~{quoted_amount_out:.8} {token}");
            log_run(mode, token, undead_balance, token_balance, token_value, swap_amount, "WOULD_SWAP", quoted_amount_out, 0.0, "");
        }
        Ok(AttemptOutcome::NotCleared) => {
            println!("  ! unexpected: UNDEAD -> {token} quote didn't clear the near-zero floor.");
            log_run(mode, token, undead_balance, token_balance, token_value, swap_amount, "NOT_CLEARED", 0.0, 0.0, "");
        }
        Err(e) => {
            println!("  ! swap failed, no rebalance this cycle: {e}");
            log_run(mode, token, undead_balance, token_balance, token_value, swap_amount, &format!("FAILED: {e}"), 0.0, 0.0, "");
        }
    }

    Ok(())
}

//========================================================================================
// ----- CALCULATIONS ----------------------------------------------------------------------
//========================================================================================
/// UNDEAD-equivalent value of `token_balance`, derived from a UNDEAD -> token
/// quote (`quote_out` token for `undead_quoted` UNDEAD) rather than a direct
/// token -> UNDEAD quote -- keeps every KyberSwap call in this file one-way.
fn token_value_in_undead(token_balance: f64, undead_quoted: f64, quote_out: f64) -> f64 {
    token_balance * undead_quoted / quote_out
}

/// Splits the UNDEAD-vs-token gap in half. Negative/zero means token is
/// already at or ahead of parity -- never acted on, since this program only
/// ever swaps UNDEAD -> token.
fn compute_swap_amount(undead_balance: f64, token_value_in_undead: f64) -> f64 {
    (undead_balance - token_value_in_undead) / 2.0
}
//=========================================================================================
// ----- CLI --------------------------------------------------------------------------------
///========================================================================================
#[derive(Debug, Parser)]
#[command(name = "maegen")]
#[command(version = "0.4.0")]
struct Args {
    blackchain: String,
    /// non-UNDEAD side of the pair, e.g. `BTC` -- must be in tokens.toml
    token: String,
    /// defaults to tva's wallet (the vault) -- override to run against any
    /// other wallet, as long as its tokens are in tokens.toml
    #[arg(long, env = "TVA_WALLET_ADDRESS")]
    wallet_address: String,
    /// defaults to tva's keystore -- override alongside --wallet-address
    #[arg(long, env = "TVA_KEYSTORE_PATH")]
    keystore_path: String,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(long, default_value_t = false)]
    debug: bool,
    #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
    slippage_bps: u16,
}
// ----- runoff_with_args -------------------------------------------------------------------
pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    run_cycle(&args.token, &args.wallet_address, &args.keystore_path, args.slippage_bps, args.dry_run, args.debug).await
}
//==========================================================================================
// ----- UNIT TESTS --------------------------------------------------------------------------
//==========================================================================================
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
    fn test_token_value_in_undead_uses_the_undead_to_token_rate() {
        // 1.00 UNDEAD quotes to 2.5 BTC right now -> holding 1.0 BTC is
        // worth 0.40 UNDEAD.
        assert!((token_value_in_undead(1.0, 1.00, 2.5) - 0.40).abs() < 1e-9);
    }

    #[test]
    fn test_swap_amount_math_splits_the_gap_in_half() {
        assert!((compute_swap_amount(1.00, 0.40) - 0.30).abs() < 1e-9);
    }

    #[test]
    fn test_swap_amount_is_negative_when_token_already_ahead() {
        assert!(compute_swap_amount(1.00, 1.50) <= 0.0);
    }

    #[test]
    fn test_swap_amount_is_zero_when_exactly_balanced() {
        assert!(compute_swap_amount(1.00, 1.00).abs() < 1e-9);
    }

    #[test]
    fn test_swap_amount_when_token_balance_is_zero_moves_half_of_undead() {
        assert!((compute_swap_amount(2.42, 0.0) - 1.21).abs() < 1e-9);
    }
}

//============================================================================================
// ----- FUNCTIONAL TEST -----------------------------------------------------------------------
//============================================================================================
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };


    create_testing!("quiz01::maegen");

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

    run!("live_undead_to_btc_quote", " (real KyberSwap route, UNDEAD -> BTC only, read-only)", {
        let registry = load_token_registry()?;
        let quote = now(kyber_swap(&registry, "UNDEAD", "BTC", 500.0))?;
        println!("\t500 UNDEAD quotes to ~{:.8} BTC right now", quote.amount_out);
    });

    run!("cycle_dry_run", " (real balances + quotes, never touches keystore)", {
        now(run_cycle("BTC", "0x6700bD7EAE41434f566e48738813fC585B95669a", "", DEFAULT_SLIPPAGE_BPS, true, false))?;
        println!("\tdry-run cycle completed without touching the keystore");
    });
}
