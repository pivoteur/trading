use chrono::NaiveDate;
use clap::Parser;

use book::{
    parse_args_add_banner,
    cli_utils::generate_banner,
    csv_utils::list_csv,
    err_utils::ErrStr
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{ types::wallets::Wallet, wallets::factory::connect_wallet };

//----- CLI -------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "1.3.0")]
struct Args {
    /// The wallet to read. Required -- no env fallback.
    wallet_address: String,

    #[arg(long, env = "LE_DATE")]
    date: NaiveDate,

    /// Which chain's data/{blockchain}.toml to load.
    #[arg(long, default_value_t = AVALANCHE)]
    blockchain: Blockchain,

    /// Print debugging information
    #[arg(short, long)]
    debug: bool
}

//----- Wallet Read ---------------------------------------------

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let mu_keystore = "xyz"; // we only read from this wallet
    let wallet =
       connect_wallet(&args.blockchain, &args.wallet_address, mu_keystore,
                      args.debug).await?;
    runoff_continuation(&wallet, &args.date).await
}

async fn runoff_continuation(wallet: &Box<dyn Wallet>, date: &NaiveDate)
      -> ErrStr<()> {
   let balances = wallet.balances(date).await?;
   println!("{}", list_csv(&[balances], true));
   Ok(())
}

//----- FUNCTIONAL TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, date_utils::yesterday, utils::now };
    use libs::types::blockchains::Blockchain::BINANCE;
    use trading::wallets::mock::mock_connection;

    create_testing!("quiz01::b_gelic");

    run!("gelic_avalanche", {
        let wallet = mock_connection(&AVALANCHE, true);
        let yday = &yesterday();
        now(runoff_continuation(&wallet, yday))?
    });

    run!("gelic_binance", {
        let wallet = mock_connection(&BINANCE, true);
        let yday = &yesterday();
        now(runoff_continuation(&wallet, yday))?
    });
}
