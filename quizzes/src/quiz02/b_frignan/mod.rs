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
#[command(bin_name = "frignan")]
#[command(version = "0.2.0")]
struct Args {
    from_token: UppercaseString,
    amount: f64,
    blockchain: String,

    #[arg(short, long)]
    debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let blockchains: HashMap<String, String> =
       [("binance", "bsc") ].into_iter().map(|(a,b)| (s(a), s(b))).collect();
    fn lookup(h: &HashMap<String, String>) -> impl Fn(&str) -> String + '_ {
        move |k| h.get(k).cloned().unwrap_or_else(|| k.to_string())
    }
  let args = parse_args_add_banner!(Args);
  let wallet_addy = get_env("WALLET_ADDRESS")?;
  let keystore_addy = get_env("TVA_KEYSTORE_PATH")?;
  let tokens = read_file(&format!("{}.toml", args.blockchain))?;
  let registry = load_token_registry(&tokens)?;
  let block = lookup(&blockchains);
  let ans = attempt_trade_with_actual_amount(&block(&args.blockchain), &wallet_addy, &registry, &args.from_token, "USDC", args.amount, 0.0, 1000, &keystore_addy, true, args.debug).await;
  println!("I gotch you, {ans:?}");
  Ok(())
}
