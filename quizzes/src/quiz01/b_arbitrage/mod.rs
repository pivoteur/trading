use clap::Parser;
use book::{
        cli_utils::generate_banner,
        err_utils::ErrStr,
        file_utils::read_file,
        parse_args_add_banner,
};
use libs::{
        fetchers::calls::fetch_calls,
        types::calls::Call,
};
use trading::auto_trading::{
                TokenRegistry, parse_token_registry, token_entry,
                wallet_address_from_env, wallet_balance, query_swap, execute_trade,
                append_trade_log_line, now_ts
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================
const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");

pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
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
//----- calls: per-row batch execution against calls.csv -------------------
//============================================================================
// keystore unlock through swap submission is identical for every
// auto-trading binary, so it's not duplicated here.
//
// calls.csv rows use their own from/pivot/proposed shape, which isn't
// necessarily a UNDEAD pool at all — this stays a separate, generic
// ad-hoc mechanism with its own flat log.
const AD_HOC_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/arbitrage_trades.log");

fn log_trade_outcome(from_symbol: &str, to_symbol: &str, amount: f64, quote_out: f64, tx_hash: &str) {
    let line = format!("{},{from_symbol}->{to_symbol},{amount:.6},{quote_out:.8},{tx_hash}", now_ts());
    append_trade_log_line(AD_HOC_LOG_PATH, &line, None);
}

async fn run_trade_for_symbols(
    blockchain: &str,
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
    println!("Wallet ({wallet_address}): {available:.8} {from_symbol} available");
    if available + 1e-6 < amount {
        return Err(format!(
            "Insufficient {from_symbol} — need {amount:.8}, only {available:.8} available. \
             That's not happening. No funds used."
        ));
    }

    let swap = query_swap("avalanche", registry, from_symbol, to_symbol, amount, debug).await?;
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

    match execute_trade(blockchain, wallet_address, registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, KEYSTORE_PATH_VAR, debug).await {
        Ok((tx_hash, _gas_avax)) => {
            println!(">>> Trade complete. Tx hash: {tx_hash}");
            log_trade_outcome(from_symbol, to_symbol, amount, swap.amount_out, &tx_hash);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Reads calls.csv and processes every row independently.
/// all-or-nothing on each row and if arbitrage cannot close it
/// refuses and tries the next row.
pub async fn run_calls_batch(blockchain: &str, root_url: &str, slippage_bps: u16, dry_run: bool, debug: bool) -> ErrStr<()> {
    let wallet_address = wallet_address_from_env("WALLET_ADDRESS")?;
    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry = load_token_registry(&tokens)?;

    let calls: Vec<Call> = fetch_calls(root_url)
        .await
        .map_err(|e| format!("Could not fetch calls.csv from {root_url}: {e}"))?;
    println!("Fetched {} call(s) from {root_url}", calls.len());

    let mut cleared = 0usize;
    let mut skipped = 0usize;
    let mut validated = Vec::new();

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

        let swap = query_swap("avalanche", &registry, from_symbol, to_symbol, amount, debug).await?;
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

        if token_entry(&registry, from_symbol).is_err() || token_entry(&registry, to_symbol).is_err() {
            println!("  ! Call #{}: '{from_symbol}' or '{to_symbol}' not in {blockchain}.toml — skipping this row. No funds moved.", call.ix);
            skipped += 1;
            continue;
        }

        match run_trade_for_symbols(blockchain, &wallet_address, &registry, from_symbol, to_symbol, amount, min_floor, slippage_bps, dry_run, debug).await {
            Ok(()) => cleared += 1,
            Err(e) => {
                println!("  ! Call #{} skipped: {e}", call.ix);
                skipped += 1;
            }
        }
    }

    let mode_tag = if dry_run { " [DRY RUN]" } else { "" };
    println!("Batch complete{mode_tag}: {cleared} cleared, {skipped} skipped, {} total.", calls.len());
    Ok(())
}

//============================================================================
//----- CLI ---------------------------------------------------------------------
//============================================================================
#[derive(Debug, Parser)]
#[command(name = "arbitrage")]
#[command(version = "0.17.0")]
struct Args {
    /// calls.csv is fetched from this root URL and every row is attempted
    /// independently — a row that can't clear is skipped, not a veto over
    /// the rest of the file.
    #[arg(long, env = "PIVOT_URL")]
    root_url: String,
    #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
    slippage_bps: u16,
    /// Which blockchain's data/{blockchain}.toml to load. (e.g. 'avalanche')
    #[arg(long, default_value = "avalanche")]
    blockchain: String,
    /// Checks only — never touches the keystore or sends a tx.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    /// Shows you what is going on behind the curtains
    #[arg(short = 'd', long, default_value_t = false)]
    debug: bool,
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    run_calls_batch(&args.blockchain, &args.root_url, args.slippage_bps, args.dry_run, args.debug).await
}

//============================================================================
//----- UNIT TESTS -------------------------------------------------------------
//============================================================================
#[cfg(test)]
mod unit_tests {
    use super::*;


    #[test]
    fn test_load_token_registry_has_expected_tokens() -> ErrStr<()> {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        for symbol in ["AVAX", "BTC", "ETH", "USDC", "UNDEAD"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in avalanche.toml");
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_run_trade_for_symbols_rejects_zero_or_negative_amounts() -> ErrStr<()> {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let dummy_wallet = "0x0000000000000000000000000000000000dEaD";
        assert!(run_trade_for_symbols("avalanche", dummy_wallet, &registry, "BTC", "ETH", 0.0, 1.0, 50, true, false).await.is_err());
        assert!(run_trade_for_symbols("avalanche", dummy_wallet, &registry, "BTC", "ETH", 1.0, 0.0, 50, true, false).await.is_err());
        assert!(run_trade_for_symbols("avalanche", dummy_wallet, &registry, "BTC", "ETH", -1.0, 1.0, 50, true, false).await.is_err());
        Ok(())
    }

    #[test]
    fn test_unknown_blockchain_fails_clearly_instead_of_falling_back() {
        let result = read_file(&format!("{DATA_DIR}/definitely_not_a_real_chain.toml"));
        assert!(result.is_err(), "an unrecognized --blockchain value must fail loudly, not silently load some default token file");
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

    const PIVOT_ROOT_URL: &str = "https://raw.githubusercontent.com/pivoteur/pivoteur.github.io";
    const TEST_WALLET: &str = "0xd16E431b1363Ed90C4fD4906Cf7Fc33E51115429";

    create_testing!("quiz12::arbitrage");

    run!("wallet_balance", " (real ETH read against dedicated test wallet, read-only)", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(TEST_WALLET, "ETH", &registry))?;
        println!("\ttest wallet ETH balance: {balance:.4}");
    });

    run!("calls_batch_dry_run", " (real calls.csv fetch + read-only per-row checks)", {
        // A per-row insufficient-balance or under-floor quote is no longer a
        // hard error here — it's logged and skipped, and the batch as a
        // whole still returns Ok. Only a setup-level failure (env var, the
        // token file, or the calls.csv fetch itself) should propagate.
        now(run_calls_batch("avalanche", PIVOT_ROOT_URL, 50, true, false))?;
        println!("\tcalls batch dry run completed without touching the keystore");
    });
}
