use std::collections::HashMap;
use trading::auto_trading::{
            attempt_trade_with_actual_amount, 
            TokenRegistry,
            parse_token_registry
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


#[derive(Debug, Parser)]
#[command(name = "frignan")]
#[command(version = "1.0.3")]
struct Args {
    from_token: UppercaseString,
    amount: f64,
    blockchain: String,

    #[arg(short, long)]
    debug: bool
}

async fn runoff_continuation(blockchain: &str, from_token: &str, amount: f64, debug: bool) -> ErrStr<()> {
    let blockchains: HashMap<String, String> =
       [("binance", "bsc") ].into_iter().map(|(a,b)| (s(a), s(b))).collect();
    fn lookup(h: &HashMap<String, String>) -> impl Fn(&str) -> String + '_ {
        move |k| h.get(k).cloned().unwrap_or_else(|| k.to_string())
    }
    let wallet_addy = get_env("WALLET_ADDRESS")?;
    let keystore_addy = get_env("TVA_KEYSTORE_PATH")?;
    let tokens = read_file(&format!("data/{}.toml", blockchain))?;
    let registry = load_token_registry(&tokens)?;
    let block = lookup(&blockchains);
    let ans = attempt_trade_with_actual_amount(&block(&blockchain), &wallet_addy, &registry, &from_token, "USDC", amount, 0.0, 1000, &keystore_addy, true, debug).await;
    println!("I gotch you, {ans:?}");
    Ok(())
}

pub async fn runoff_with_args() -> ErrStr<()> {
  let args = parse_args_add_banner!(Args);
  runoff_continuation(&args.blockchain, &args.from_token, args.amount, args.debug).await
}

//=========================================================================
// ----- FUNCTIONAL TESTS --------------------------------------------------
//=========================================================================
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_test { 
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };


    create_testing!("quiz02::b_frignan");

    run!("frignan_functionailty", {
        now(runoff_continuation("avalanche", "BTC", 1.0, true))?
    });
}
