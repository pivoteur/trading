use clap::Parser;

use book::{
    parse_args_add_banner,
    cli_utils::generate_banner,
    csv_utils::as_csv,
    err_utils::ErrStr
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{ types::wallets::Wallet, wallets::factory::connect_wallet };

//----- CLI -------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "1.2.1")]
struct Args {
    /// The wallet to read. Required -- no env fallback.
    wallet_address: String,

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
    runoff_continuation(&wallet).await
}

async fn runoff_continuation<W: Wallet>(wallet: &W) -> ErrStr<()> {
   let balances = wallet.balances().await?;
    println!("{}", as_csv(&balances, true)?);
    Ok(())
}

//----- FUNCTIONAL TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use libs::types::blockchains::Blockchain::BINANCE;
    use trading::wallets::mock::mock_connection;

    create_testing!("quiz01::b_gelic");

    run!("gelic_avalanche", {
        let wallet = mock_connection(&AVALANCHE, true);
        now(runoff_continuation(&wallet))?
    });

    run!("gelic_binance", {
        let wallet = mock_connection(&BINANCE, true);
        now(runoff_continuation(&wallet))?
    });
}
