use trading::{ types::wallets::Wallet, wallets::mock::mock_connection };
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
#[command(version = "1.3.1")]
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
  let wallet = mock_connection(&args.blockchain, args.debug);
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
    use libs::types::blockchains::Blockchain::BINANCE;

    create_testing!("quiz01::a_frignan");

    run!("btc", {
        let wallet = mock_connection(&AVALANCHE, true);
        now(runoff_continuation(&wallet, "BTC"))?
    });

    run!("undead", {
        let wallet = mock_connection(&AVALANCHE, false);
        now(runoff_continuation(&wallet, "UNDEAD"))?
    });

    run!("bnb", {
        let wallet = mock_connection(&BINANCE, true);
        now(runoff_continuation(&wallet, "BNB"))?
    });

    run!("doge", {
        let wallet = mock_connection(&BINANCE, true);
        now(runoff_continuation(&wallet, "DOGE"))?
    });

    run!("ltc", {
        let wallet = mock_connection(&BINANCE, true);
        now(runoff_continuation(&wallet, "ltc"))?
    });

    run!("link", {
        let wallet = mock_connection(&BINANCE, true);
        now(runoff_continuation(&wallet, "link"))?
    });
}
