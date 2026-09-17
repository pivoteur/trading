use trading::{
   auto_trading::attempt_trade_with_actual_amount,
   fetchers::tokens::fetch_tokens
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use clap::Parser;

//=================================================================
// ----- CLI -----------------------------------------------------
//===============================================================
#[derive(Debug, Parser)]
#[command(name = "ceap")]
#[command(version = "1.1.1")]
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
    runoff_continuation(&args.blockchain, &args.wallet_address,
                        &args.keystore_path,
                        &args.from_token, &args.to_token, amount as f64,
                        floor as f64, args.dry_run, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, addy: &str,
                             keystore_path: &str, from: &str, to: &str,
                             amount: f64, floor: f64,
                             dry_run: bool, debug: bool) -> ErrStr<()> {
    let registry = fetch_tokens(blockchain).await?;
    let ans =
        attempt_trade_with_actual_amount(blockchain, addy, &registry, from, to,
                                         amount, floor, 1000, keystore_path,
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

    run!("ceap_functionality",
        now(runoff_continuation(&AVALANCHE, "0x123", "xyz",
                                "BTC", "ETH", 1.0, 0.0, true, true))?);
}
