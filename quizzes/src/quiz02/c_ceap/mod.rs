use std::collections::HashMap;
use trading::auto_trading::{
    attempt_trade_with_actual_amount,
    resolve_wallet_address,
    TokenRegistry,
    parse_token_registry,
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   string_utils::{UppercaseString,s},
   file_utils::read_file
};
use clap::Parser;


const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");

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
    /// CLI override; falls back to WALLET_ADDRESS via `resolve_wallet_address`.
    #[arg(long)]
    wallet_address: Option<String>,
    #[arg(long, env = "TVA_KEYSTORE_PATH")]
    keystore_path: String,
    /// Force a dry run even if --live is also passed.
    #[arg(long)]
    dry_run: bool,
    #[arg(short = 'd', long)]
    debug: bool,
}

#[allow(clippy::too_many_arguments)]
async fn runoff_continuation(blockchain: &str, from_token: &str, to_token: &str, amount: f64, floor: Option<f64>, wallet_address: &str, keystore_path: &str, dry_run: bool, debug: bool) -> ErrStr<()> {
    let blockchains: HashMap<String, String> =
        [("binance", "bsc")].into_iter().map(|(a, b)| (s(a), s(b))).collect();
    let floor = floor.unwrap_or(0.0);
    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry = load_token_registry(&tokens)?;
    let block = lookup(&blockchains);

    let ans = attempt_trade_with_actual_amount(
        &block(blockchain), wallet_address, &registry, from_token, to_token, amount, floor, 1000, keystore_path, dry_run, debug).await;

    println!("answer is {ans:?}");
    Ok(())

}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let wallet_address = resolve_wallet_address(args.wallet_address, "WALLET_ADDRESS")?;
    runoff_continuation(&args.blockchain, &args.from_token, &args.to_token, args.amount, args.floor, &wallet_address, &args.keystore_path, args.dry_run, args.debug).await
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
        now(runoff_continuation("avalanche", "BTC", "ETH", 1.0, None, "0x123", "unused-in-dry-run", true, true))?
    });
}
