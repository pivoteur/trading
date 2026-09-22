use trading::{
   auto_trading::query_quote,
   fetchers::tokens::fetch_token_registry
};
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   string_utils::UppercaseString
};
use clap::Parser;
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };

//========================================================
// ----- CLI ---------------------------------------------
//========================================================
#[derive(Debug, Parser)]
#[command(name = "frignan")]
#[command(version = "1.2.2")]
struct Args {
    /// The token you want to see the current price of.
    token: UppercaseString,

    /// The <blockchain>.toml to load
    #[arg(long, default_value_t = AVALANCHE)]
    blockchain: Blockchain,

    /// To see what is going on behind the scenes.
    #[arg(short, long)]
    debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
  let args = parse_args_add_banner!(Args);
  runoff_continuation(&args.blockchain, &args.token, args.debug).await
}

async fn runoff_continuation(blockchain: &Blockchain, from_token: &str,
                             debug: bool) -> ErrStr<()> {
    let registry = fetch_token_registry(blockchain).await?;
    let quote =
       query_quote(blockchain, &registry, from_token, debug).await?;
    println!("{from_token}'s price is {quote}");
    Ok(())
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

    create_testing!("quiz01::a_frignan");

    run!("frignan_btc", {
        now(runoff_continuation(&AVALANCHE, "BTC", true))?
    });

    run!("frignan_undead", {
        now(runoff_continuation(&AVALANCHE, "UNDEAD", false))?
    });
}
