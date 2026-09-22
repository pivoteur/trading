use clap::Parser;

use trading::{
   auto_trading::attempt_trade_with_actual_amount,
   fetchers::tokens::fetch_token_registry,
   types::tokens::TokenRegistry
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };

//=================================================================
// ----- CLI -----------------------------------------------------
//===============================================================
#[derive(Debug, Parser)]
#[command(name = "ceap")]
#[command(version = "1.1.5")]
struct Args {
    /// trading from this token
    from_token: UppercaseString,

    /// amount of `from_token` to trade
    amount: CommaFloat,

    /// trading to this token
    to_token: UppercaseString,

    /// Minimum acceptable output amount.
    #[arg(long, default_value_t = CommaFloat(0.0))]
    floor: CommaFloat,

    /// Slippage percent (where 1000 BPS is 10% higher than floor)
    #[arg(long, default_value_t = 0)]
    slippage_bps: u16,

    /// wallet address on which trade occurs
    #[arg(long, env="WALLET_ADDRESS")]
    wallet_address: String,

    /// Keystore to wallet: permits trades on this wallet
    #[arg(long, env = "KEYSTORE_PATH")]
    keystore_path: String,

    /// blockchain on which to execute trade
    #[arg(long, default_value_t = AVALANCHE)]
    blockchain: Blockchain,

    /// Force a dry run even if --live is also passed.
    #[arg(long)]
    dry_run: bool,

    /// Show debugging information
    #[arg(short, long)]
    debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let amount: f32 = args.amount.into();
    let floor: f32 = args.floor.into();
    let chain = &args.blockchain;
    let registry = fetch_token_registry(chain).await?;
    runoff_continuation(chain, &args.wallet_address,
                        &args.keystore_path, &registry,
                        &args.from_token, &args.to_token, amount,
                        floor, args.slippage_bps,
                        args.dry_run, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, addy: &str,
                             keystore_path: &str, registry: &TokenRegistry,
                             from: &str, to: &str,
                             amount: f32, floor: f32, slippage: u16,
                             dry_run: bool, debug: bool) -> ErrStr<()> {
    let ans =
        attempt_trade_with_actual_amount(blockchain, addy, &registry, from, to,
                                         amount, floor, slippage, keystore_path,
                                         dry_run, debug).await?;
    println!("answer is {ans:?}");
    Ok(())
}

// =======================================================================
// ----- FUNCTIONAL TESTS --------------------------------------------------
// =======================================================================
#[cfg(not(tarpaulin_include))]
#[cfg(test)]
pub mod functional_test {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };

    create_testing!("quiz02::c_ceap");

    run!("ceap", {
        let ava = &AVALANCHE;
        let registry = now(fetch_token_registry(ava))?;
        now(runoff_continuation(ava, "0x123", "xyz", &registry,
                                "BTC", "ETH", 1.0, 16.0, 200, true, true))?;
    });
}
