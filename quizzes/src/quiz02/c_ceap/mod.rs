use std::collections::HashMap;
use trading::auto_trading::{
    attempt_trade_with_actual_amount,
    TokenRegistry,
    parse_token_registry,
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   string_utils::{UppercaseString,s},
   utils::get_env,
   file_utils::read_file
};
use clap::Parser;


pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}
fn lookup(h: &HashMap<String, String>) -> impl Fn(&str) -> String + '_ {
    move |k| h.get(k).cloned().unwrap_or_else(|| k.to_string())
}
//=========================================================================================
// ----- CLI --------------------------------------------------------------------------------
//=========================================================================================
#[derive(Debug, Parser)]
#[command(name = "ceap")]
#[command(version = "0.1.1")]
struct Args {
    blockchain: String,
    from_token: UppercaseString,
    amount: f64,
    to_token: UppercaseString,
    /// Minimum acceptable output amount. Required when --live is set.
    #[arg(long)]
    floor: Option<f64>,
    /// Force a dry run even if --live is also passed.
    #[arg(long)]
    dry_run: bool,
    #[arg(short, long)]
    debug: bool,
}

async fn runoff_continuation(blockchain: &str, from_token: &str, to_token: &str, amount: f64, floor: Option<f64>, dry_run: bool, debug: bool) -> ErrStr<()> {
    let blockchains: HashMap<String, String> =
        [("binance", "bsc")].into_iter().map(|(a, b)| (s(a), s(b))).collect();
    let floor = floor.unwrap_or(0.0);
    let wallet_addy = get_env("WALLET_ADDRESS")?;
    let keystore_addy = get_env("TVA_KEYSTORE_PATH")?;
    let tokens = read_file(&format!("data/{}.toml", blockchain))?;
    let registry = load_token_registry(&tokens)?;
    let block = lookup(&blockchains);
        
    let ans = attempt_trade_with_actual_amount(
        &block(blockchain), &wallet_addy, &registry, from_token, to_token, amount, floor, 1000, &keystore_addy, dry_run, debug).await;
        
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
