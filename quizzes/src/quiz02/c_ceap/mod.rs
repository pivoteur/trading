use std::collections::HashMap;
use trading::{
   auto_trading::attempt_trade_with_actual_amount,
   tokens::{ TokenRegistry, load_tokens }
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   string_utils::{UppercaseString,s},
   utils::get_env,
   file_utils::read_file
};
use libs::types::blockchains::Blockchain;
use clap::Parser;

//=========================================================================================
// ----- CLI --------------------------------------------------------------------------------
//=========================================================================================
#[derive(Debug, Parser)]
#[command(name = "ceap")]
#[command(version = "0.1.1")]
struct Args {
    /// trading from this token
    from_token: UppercaseString,
    /// amount of `from_token` to trade
    amount: f64,
    /// trading to this token
    to_token: UppercaseString,
    /// Minimum acceptable output amount. Required when --live is set.
    #[arg(long)]
    floor: Option<f64>,
    /// blockchain on which to execute trade
    #[arg(long, default_value_t = AVALANCHE)]
    blockchain: Blockchain,
    /// Force a dry run even if --live is also passed.
    #[arg(long)]
    dry_run: bool,
    /// Show debugging information
    #[arg(short, long)]
    debug: bool,
}

async fn runoff_continuation(blockchain: &Blockchain, from_token: &str, to_token: &str, amount: f64, floor: Option<f64>, dry_run: bool, debug: bool) -> ErrStr<()> {
    let floor = floor.unwrap_or(0.0);
    let registry = load_token_registry(blockchain)?;
    let ans = attempt_trade_with_actual_amount(
        blockchain, &wallet_addy, &registry, from_token, to_token, amount, floor, 1000, &keystore_path, dry_run, debug).await;
    println!("answer is {ans:?}");
    Ok(())
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    runoff_continuation(&args.blockchain, &args.from_token, &args.to_token, args.amount, args.floor, args.dry_run, args.debug).await
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

    run!("ceap_functionality", {
        now(runoff_continuation("avalanche", "BTC", "ETH", 1.0, None, true, true))?
    });
}
