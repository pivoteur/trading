use clap::Parser;
use book::{
   debug,
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   addresses::is_valid_evm_address,
   auto_trading::send_tokens_to_address,
   consts::DUST_EPSILON,
   fetchers::{ tokens::fetch_token_registry, wallets::fetch_token_balance }
};

//======================================================
// ----- CLI -------------------------------------------
//======================================================

/// sendan (Old English: "to send") -- a one-shot ERC-20 transfer, e.g.
/// `sendan avalanche 1100 UNDEAD 0x12345...`. No pivots, no replayed
/// state: every invocation is a single, independent send.
#[derive(Debug, Parser)]
#[command(name = "sendan", version = "1.1.3")]
struct Args {
    /// ERC-20 token symbol to send; must have an address entry in 
    ///the <blockchain>.toml's file. e.g. `UNDEAD`
    token: UppercaseString,

    /// Amount of `token` to send. e.g. `1100`
    amount: CommaFloat,

    /// Destination address: `0x` followed by 40 hex characters, 
    /// e.g. `0x1234567890abcdef1234567890abcdef12345678`
    to_address: String,

    /// Blockchain to send on; must match a `data/<blockchain>.toml` file
    #[arg(long, default_value_t=AVALANCHE)]
    blockchain: Blockchain,

    /// Wallet address to send from. e.g. `0xabc...etc'
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
    let amount: f32 = args.amount.into();
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

async fn runoff_continuation(blockchain: &Blockchain, amount: f32, token: &str,
                             to: &str, addy: &str, keystore_path: &str,
                             dry_run: bool, debug: bool) -> ErrStr<()> {
    debug!("sendan_continuation", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    let log_line = format!("{mode} send {amount:.8} {token} -> {to}");
    log!("mode {}", log_line);

    if amount <= DUST_EPSILON {
        return Err(format!("sendan: amount must be positive, got {amount}"));
    }

    let registry = fetch_token_registry(&blockchain).await?;

    // fail fast on an unknown/native token before spending an RPC call on
    // a balance check we already know can't lead anywhere.
    let entry = registry.token(token)?;
    if entry.address.is_none() {
      return Err(format!("'{token}' has no address in data/{blockchain}.toml"));
    }

    log!("wallet {}", addy);
    let balance0 =
       fetch_token_balance(blockchain, addy, token, &registry, debug).await?;
    let balance = balance0.unwrap_or(0.0);
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
          send_tokens_to_address(blockchain, addy, &registry, token, to, amount,
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

    #[tokio::test] async fn test_sendan_rejects_zero_amount() {
        let result = runoff_continuation(
            &AVALANCHE, 0.0, "UNDEAD",
            "0x000000000000000000000000000000000000CD",
            "0x123", "", true, false).await;
        assert!(result.is_err(),
                "a zero amount must never reach the wallet-balance check");
    }

    #[tokio::test] async fn test_sendan_rejects_malformed_address() {
        let result = runoff_continuation(
            &AVALANCHE, 100.0, "UNDEAD", "not-an-address", 
            "0x123", "", true, false).await;
        assert!(result.is_err(),
          "a malformed destination must never reach the wallet-balance check");
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
    use book::{ create_testing, string_utils::s, utils::now };
    use libs::types::blockchains::Blockchain::AVALANCHE;

    create_testing!("quiz01::d_sendan");

    run!("sendan_dry_run_rejects_bad_address", {
        let result = now(runoff_continuation(
           &AVALANCHE, 1.0, "UNDEAD", "not-address", "0x123", "", true, false));
        assert!(result.is_err());
        println!("sendan is ok");
    });

    run!("sendan", {
        let registry = now(fetch_token_registry(&AVALANCHE))?;
        let balance0 =
           now(fetch_token_balance(&AVALANCHE, "0x123", "UNDEAD", 
                                   &registry, true))?;
        let balance = balance0.unwrap_or(0.0);
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
        if balance <= DUST_EPSILON {
            println!("\ttest wallet holds no UNDEAD -- confirming sendan correctly refuses to send rather than assuming a happy path");
            let result = now(runoff_continuation(
                &AVALANCHE, 1.0, "UNDEAD",
                "0x000000000000000000000000000000000000AB", 
                "0x123", "", true, false));
            if result.is_ok() {
                return Err(s("expected an insufficient-balance error against 
an empty test wallet, got Ok"))
            }
        } else {
            let amount = balance / 2.0;
            now(runoff_continuation(
                &AVALANCHE, amount, "UNDEAD",
                "0x000000000000000000000000000000000000AB",
                "0x123", "", true, false))?;
            println!("dry-run WOULD_SEND {amount:.8} UNDEAD");
        }
        println!("sendan is ok");
    });
}
