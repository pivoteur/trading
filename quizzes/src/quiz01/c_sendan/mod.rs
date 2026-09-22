use clap::Parser;
use book::{
   debug, parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   auto_trading::send_tokens_to_address,
   fetchers::tokens::fetch_token_registry,
   types::tokens::TokenRegistry
};

//======================================================
// ----- CLI -------------------------------------------
//======================================================

/// sendan (Old English: "to send") -- a one-shot ERC-20 transfer, e.g.
/// `sendan avalanche 1100 UNDEAD 0x12345...`. No pivots, no replayed
/// state: every invocation is a single, independent send.
#[derive(Debug, Parser)]
#[command(name = "sendan", version = "1.1.4")]
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
    let chain = &args.blockchain;
    let registry = fetch_token_registry(chain).await?;
    runoff_continuation(chain, &registry, amount, &args.token, to,
                        &args.wallet_address, &args.keystore_path,
                        args.dry_run, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, registry: &TokenRegistry,
                             amount: f32, token: &str, to: &str, addy: &str,
                             keystore_path: &str, dry_run: bool, debug: bool)
      -> ErrStr<()> {
    debug!("runoff_continuation", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    let log_line = format!("{mode} send {amount:.8} {token} -> {to}");
    log!("mode {}", log_line);

    log!("wallet {}", addy);

    if !dry_run {
       let _ =
          send_tokens_to_address(blockchain, addy, &registry, token, to, amount,
                                 keystore_path, debug).await?;
    }
    Ok(())
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
    use trading::{
       consts::{ UNDEAD, test_wallets::TEST_ADDRESS },
       fetchers::wallets::fetch_token_balance
    };

    create_testing!("quiz01::c_sendan");

    run!("sendan", {
        let registry = now(fetch_token_registry(&AVALANCHE))?;
        let balance0 =
           now(fetch_token_balance(&AVALANCHE, TEST_ADDRESS, UNDEAD, 
                                   &registry, true))?;
        let balance = balance0.unwrap_or(0.0);
        println!("\ttest wallet UNDEAD balance: {balance:.0}");
        let amount = balance / 2.0;
        now(runoff_continuation(&AVALANCHE, &registry, amount, UNDEAD,
                "0x000000000000000000000000000000000000AB",
                TEST_ADDRESS, "xyz", true, true))?;
    });
}
