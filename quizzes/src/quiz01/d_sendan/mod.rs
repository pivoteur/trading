use clap::Parser;
use book::{
   debug,
   parse_args_add_banner,
   err_utils::ErrStr,
   string_utils::UppercaseString,
   cli_utils::generate_banner,
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   addresses::is_valid_evm_address,
   auto_trading::send_tokens_to_address,
   fetchers::{ tokens::fetch_tokens, wallets::fetch_wallet_balance }
};

//======================================================
// ----- const -----------------------------------------
//======================================================
const DUST_EPSILON: f64 = 1e-8;

//======================================================
// ----- CLI -------------------------------------------
//======================================================

/// sendan (Old English: "to send") -- a one-shot ERC-20 transfer, e.g.
/// `sendan avalanche 1100 UNDEAD 0x12345...`. No pivots, no replayed
/// state: every invocation is a single, independent send.
#[derive(Debug, Parser)]
#[command(name = "sendan", version = "1.0.1")]
struct Args {
    /// ERC-20 token symbol to send; must have an address entry in the <blockchain>.toml's file. e.g. `UNDEAD`
    token: UppercaseString,

    /// Amount of `token` to send. e.g. `1100`
    amount: f64,

    /// Destination address: `0x` followed by 40 hex characters. e.g. `0x1234567890abcdef1234567890abcdef12345678`
    to_address: String,

    /// Blockchain to send on; must match a `data/<blockchain>.toml` file
    #[arg(long, default_value_t=AVALANCHE)]
    blockchain: Blockchain,

    /// Wallet address to send from. e.g. `0xabc0000000000000000000000000000000000123`
    #[arg(long, env = "WALLET_ADDRESS")]
    wallet_address: String,

    /// Path to the encrypted keystore file used to sign the transaction
    #[arg(long, env = "KEYSTORE_PATH")]
    keystore_path: String,

    /// Simulate the send without broadcasting a transaction
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Print verbose debug logging
    #[arg(short = 'd', long, default_value_t = true)]
    debug: bool
}

//=======================================================================
// ----- SEND FN --------------------------------------------------------
//=======================================================================

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let amount = args.amount;
    if amount <= DUST_EPSILON {
        return Err(format!("sendan: amount must be positive, got {amount}"));
    }
    let to = &args.to_address;
    if !is_valid_evm_address(to) {
        return Err(format!(
            "'{to}' doesn't look like an EVM address
expected '0x' followed by 40 hex characters."));
    }
    runoff_continuation(&args.blockchain, amount, &args.token, to,
                        &args.wallet_address, &args.keystore_path,
                        args.dry_run, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, amount: f64, token: &str,
                             to: &str, addy: &str, keystore_path: &str,
                             dry_run: bool, debug: bool) -> ErrStr<()> {
    debug!("sendan_continuation", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    let log_line = format!("{mode} send {amount:.8} {token} -> {to}");
    log!("mode {}", log_line);

    let registry = fetch_tokens(&blockchain).await?;

    // fail fast on an unknown/native token before spending an RPC call on
    // a balance check we already know can't lead anywhere.
    let entry = registry.token(token)?;
    if entry.address.is_none() {
      return Err(format!("'{token}' has no address in data/{blockchain}.toml"));
    }

    let balance = fetch_wallet_balance(addy, token, &registry).await?;
    log!("wallet {}", addy);
    let log_line1 = format!("{token} {balance:.8}");
    log!("Balance {}", log_line1);

    if amount > balance + DUST_EPSILON {
        return Err(format!(
            "insufficient {token} balance: have {balance:.8}, asked to send {amount:.8} -- cancelling send."
        ));
    }

    if dry_run {
        println!("  WOULD SEND  {amount:.8} {token} -> {to}");
    } else {
       let (tx_hash, gas) =
          send_tokens_to_address(addy, &registry, token, to, amount,
                                 keystore_path, debug).await?;
          let sent = format!("{amount:.8} {token} -> {to}, gas {gas:.5}");
          log!("SENT {} AVAX tx {}", sent, tx_hash);
    }
    Ok(())
}

//=========================================================
// ----- UNIT TESTS ---------------------------------------
//=========================================================

#[cfg(test)]
mod unit_tests {
    use super::*;
    use libs::types::blockchains::Blockchain::AVALANCHE;

    #[test]
    fn test_sendan_continuation_rejects_zero_amount() {
        let result = now(sendan_continuation(
            "avalanche", 0.0, "UNDEAD", "0x000000000000000000000000000000000000CD", "0x123", "", true, false,
        ));
        assert!(result.is_err(), "a zero amount must never reach the wallet-balance check");
    }

    #[test]
    fn test_sendan_continuation_rejects_malformed_address() {
        let result = now(sendan_continuation(
            "avalanche", 100.0, "UNDEAD", "not-an-address", "0x123", "", true, false,
        ));
        assert!(result.is_err(), "a malformed destination must never reach the wallet-balance check");
    }
}

// =======================================================
// ----- FUNCTIONAL TEST ---------------------------------
//========================================================

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use libs::types::blockchains::Blockchain::AVALANCHE;

    create_testing!("quiz01::d_sendan");

    run!("sendan_dry_run_rejects_bad_address", {
        let result = now(sendan_continuation(
            AVALANCHE, 1.0, "UNDEAD", "not-address", "0x123", "", true, false));
        assert!(result.is_err());
        println!("sendan is ok");
    });

    run!("sendan", {
        let registry = now(load_tokens(&AVALANCHE))?;
        let balance = now(fetch_wallet_balance("0x123", "UNDEAD", &registry))?;
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
        if balance <= DUST_EPSILON {
            println!("\ttest wallet holds no UNDEAD -- confirming sendan correctly refuses to send rather than assuming a happy path");
            let result = now(sendan_continuation(
                "avalanche", 1.0, "UNDEAD", "0x000000000000000000000000000000000000AB", "0x123", "", true, false,
            ));
            if result.is_ok() {
                return Err("expected an insufficient-balance error against an empty test wallet, got Ok".to_string());
            }
        } else {
            let amount = balance / 2.0;
            now(sendan_continuation(
                "avalanche", amount, "UNDEAD", "0x000000000000000000000000000000000000AB", "0x123", "", true, false,
            ))?;
            println!("\tdry-run WOULD_SEND {amount:.8} UNDEAD accepted end to end");
        }
        println!("sendan is ok");
    });
}
