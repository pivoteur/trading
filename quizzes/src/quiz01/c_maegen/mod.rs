use clap::Parser;
use book::{
   debug,
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   string_utils::{ UppercaseString, s }
};
use libs::types::blockchains::Blockchain;
use trading::{
   auto_trading::{
      AttemptOutcome, attempt_trade_with_actual_amount, query_swap
   },
   consts::{ NO_REAL_FLOOR, UNDEAD },
   fetchers::{ tokens::fetch_tokens, wallets::fetch_wallet_balance },
   types::tokens::TokenRegistry
};

const DEFAULT_SLIPPAGE_BPS: u16 = 50;
const DUST_EPSILON: f64 = 1e-8;

// =======================================================
// ----- CLI ---------------------------------------------
//========================================================

#[derive(Debug, Parser)]
#[command(version = "1.2.0")]
struct Args {

    /// Blockchain on which trade occurs 
    blockchain: Blockchain,

    /// non-UNDEAD side of the pair, e.g. `BTC`
    token: UppercaseString,

    /// defaults to the vault -- override to run against any
    /// other wallet, as long as its tokens are in data/{blockchain}.toml
    #[arg(long, env = "VAULT_ADDRESS")]
    wallet_address: String,

    /// defaults to the vault -- override alongside --wallet-address
    #[arg(long, env = "VAULT_KEYSTORE_PATH")]
    keystore_path: String,

    /// inverse slippage allowed
    #[arg(long, default_value_t = DEFAULT_SLIPPAGE_BPS)]
    slippage_bps: u16,    

    /// dry run; do not execute
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// show debugging information
    #[arg(short = 'd', long, default_value_t = false)]
    debug: bool
}

// =======================================================
// ----- CALCULATIONS ------------------------------------
//========================================================

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

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    runoff_continuation(&args.blockchain, &args.token, &args.wallet_address,
                        &args.keystore_path, args.slippage_bps,
                        args.dry_run, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, token: &str, vault: &str,
                             keystore_path: &str, slippage_bps: u16,
                             dry_run: bool, debug: bool) -> ErrStr<()> {
    debug!("runoff_continuation", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    log!("mode {} token {}", mode, token);

    let registry = fetch_tokens(blockchain).await?;

    let undead_balance = 
       fetch_wallet_balance(blockchain, vault, UNDEAD, &registry).await?;
    let token_balance =
       fetch_wallet_balance(blockchain, vault, token, &registry).await?;
    log!("wallet {}", vault);
    log!("UNDEAD balance {}", undead_balance);
    log!("Token {} balance {}", token, token_balance);

    // swap_amount can never exceed undead_balance / 2 (token_value is never
    // negative) -- so that's the reference amount to quote for the rate,
    // never the full balance. This quote size is always a real possible
    // trade size, never a "sell everything" scenario that will never happen.
    let reference_amount = undead_balance / 2.0;
    if reference_amount <= DUST_EPSILON {
        Err(s("no UNDEAD to work with -- nothing to swap."))
    } else {
       let ratio = check_trade_amount(blockchain, &registry, token, 
                                      reference_amount, debug).await?;
       let swap_amount =
          check_parity(token, token_balance, reference_amount, ratio, debug)?;
       swap(blockchain, vault, &registry, slippage_bps, keystore_path, token,
            swap_amount, dry_run, debug).await
    }
}

async fn check_trade_amount(blockchain: &Blockchain, registry: &TokenRegistry,
                            token: &str, reference_amount: f64,
                            debug: bool) -> ErrStr<f64> {
   debug!("check_trade_amount", debug);
    let reference_ratio =
       query_swap(blockchain, &registry, UNDEAD, token,
                  reference_amount, debug).await?.amount_out;
    log!(" {} UNDEAD (half balance) ratio is {} {}",
         reference_amount, reference_ratio, token);
    if reference_ratio <= DUST_EPSILON {
        let line1 =
           format!("KyberSwap quoted ~0 {token} for {:.8} UNDEAD",
                   reference_amount);
        Err(format!("{line1}\n -- no route/liquidity right now."))
    } else {
       Ok(reference_ratio)
    }
}

// is it undead balance, as coded? or is it the reference_amount, which
// has been computed?

// TODO: needs testing

fn check_parity(token: &str, token_balance: f64, reference_amount: f64,
                reference_ratio: f64, debug: bool) -> ErrStr<f64> {
    debug!("check_parity", debug);
    let token_value =
       token_value_in_undead(token_balance, reference_amount, reference_ratio);
    log!("Token {} is worth {} UNDEAD right now", token, token_value);

    let swap_amount = compute_swap_amount(reference_amount, token_value);

    if swap_amount <= DUST_EPSILON {
        let line1 = format!("{token} already at/ahead of parity.");
        let line2 =
           format!("({token_value:.4} UNDEAD-equiv vs {reference_amount:.4}");
        log!("Token {}\n{} UNDEAD held)", line1, line2);
        Err(s("no swap."))
    } else {
        Ok(swap_amount)
    }
}

async fn swap(blockchain: &Blockchain, vault: &str, registry: &TokenRegistry,
              slippage_bps: u16, keystore_path: &str, token: &str,
              swap_amount: f64, dry_run: bool, debug: bool) -> ErrStr<()> {
   debug!("swap", debug);
   let from = format!("{swap_amount} UNDEAD ->");
    log!("swapping {} {}", from, token);

    match attempt_trade_with_actual_amount(blockchain, vault, &registry, UNDEAD,
                                           token, swap_amount, NO_REAL_FLOOR,
                                           slippage_bps, keystore_path,
                                           dry_run, debug).await {
       Ok(AttemptOutcome::Executed { tx_hash, actual_received, gas_avax }) => {
          let line1 =
            format!("{from} {actual_received:.8} {token}");
          let line2 = format!("gas {gas_avax:.5} AVAX   tx {tx_hash}");
            log!("SWAPPED {} {}", line1, line2);
          Ok(())
        }
        Ok(AttemptOutcome::DryRunWouldClear { quoted_amount_out }) => {
            let to = format!("{quoted_amount_out} {token}");
            log!("WOULD SWAP  {} ~{}", from, to);
          Ok(())
        }
        Ok(AttemptOutcome::NotCleared) => {
            log!("unexpected: UNDEAD -> {} quote didn't clear the floor.",
                 token);
          Ok(())
        }
        Err(e) => {
            log!(" ! swap failed, no rebalance this cycle");
            Err(e)
        }
    }
}

// ======================================================
// ----- UNIT TESTS -------------------------------------
// ======================================================

#[cfg(not(tarpaulin_include))]
#[cfg(test)]
mod unit_tests {
    use super::*;

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
// ======================================================
// ----- FUNCTIONAL TEST --------------------------------
// ======================================================
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use libs::types::blockchains::Blockchain::AVALANCHE;

    create_testing!("quiz01::c_maegen");

    run!("maegen_no_undead", {
        let _ = now(runoff_continuation(&AVALANCHE, "BTC", "0x123", "",
                                        DEFAULT_SLIPPAGE_BPS, true, false));
        println!("maegen is ok");
    });
}
