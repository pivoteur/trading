use std::{collections::HashMap, println};
use trading::auto_trading::{
                TokenRegistry, 
                query_swap,
                parse_token_registry
};
use book::{
        parse_args_add_banner,
        cli_utils::generate_banner,
        err_utils::ErrStr,
        string_utils::{UppercaseString, s},
        file_utils::read_file,
        currency::usd::mk_usd
};
use clap::Parser;


pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}
//=========================================================================================
// ----- CLI --------------------------------------------------------------------------------
//=========================================================================================
#[derive(Debug, Parser)]
#[command(name = "frignan")]
#[command(version = "1.1.5")]
struct Args {
    /// The token you want to see the current price of.
    token: UppercaseString,
    /// The <blockchain>.toml to load
    #[arg(long, default_value_t = s("avalanche"))]
    blockchain: String,
    /// To see what is going on behind the scenes.
    #[arg(short, long)]
    debug: bool
}

async fn runoff_continuation(blockchain: &str, from_token: &str, debug: bool) -> ErrStr<()> {
    let blockchains: HashMap<String, String> =
       [("binance", "bsc") ].into_iter().map(|(a,b)| (s(a), s(b))).collect();
    fn lookup(h: &HashMap<String, String>) -> impl Fn(&str) -> String + '_ {
        move |k| h.get(k).cloned().unwrap_or_else(|| k.to_string())
    }
    let tokens = read_file(&format!("data/{}.toml", blockchain))?;
    let registry = load_token_registry(&tokens)?;
    let block = lookup(&blockchains);
    let ans = query_swap(&block(&blockchain), &registry, &from_token, "USDC", 1.0, debug).await?;
    let quoted_amount_out = ans.amount_out;
    let price = mk_usd(quoted_amount_out as f32);
    println!("{from_token}'s price is {price}");
    Ok(())
}

pub async fn runoff_with_args() -> ErrStr<()> {
  let args = parse_args_add_banner!(Args);
  runoff_continuation(&args.blockchain, &args.token, args.debug).await
}

//=========================================================================
// ----- FUNCTIONAL TESTS --------------------------------------------------
//=========================================================================
#[cfg(not(tarpaulin_include))]
#[cfg(test)]
pub mod functional_test {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };


    create_testing!("quiz02::b_frignan");

    run!("frignan_functionailty", {
        now(runoff_continuation("avalanche", "BTC", true))?
    });

    run!("frignan_undead", {
        now(runoff_continuation("avalanche", "UNDEAD", false))?
    });
}
