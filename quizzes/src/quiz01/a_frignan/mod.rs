use trading::{
   types::wallets::Wallet,
   wallets::avalanche::mocks::connect_to_avalanche
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
#[command(version = "1.3.0")]
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
  // TODO: FIXME currently only works with avalanche!!!
  let wallet = connect_to_avalanche("0x1213", "abc", args.debug).await?;
  runoff_continuation(&wallet, &args.token).await
}

async fn runoff_continuation<W: Wallet>(wallet: &W, token: &str) -> ErrStr<()> {
   let quote = wallet.quote(token).await?;
   println!("{token}'s price is {quote}");
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
        let wallet = now(connect_to_avalanche("0x123", "sdf", true))?;
        now(runoff_continuation(&wallet, "BTC"))?
    });

    run!("frignan_undead", {
        let wallet = now(connect_to_avalanche("0x123", "sdf", false))?;
        now(runoff_continuation(&wallet, "UNDEAD"))?
    });
}
