use clap::Parser;

use book::{
    parse_args_add_banner,
    cli_utils::generate_banner,
    csv_utils::as_csv,
    err_utils::ErrStr
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::fetchers::{
   tokens::fetch_token_registry,
   wallets::fetch_wallet_balances
};

//----- CLI -------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "1.1.4")]
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
    runoff_continuation(&args.blockchain, &args.wallet_address,
                        args.debug).await
}

async fn runoff_continuation(b: &Blockchain, addy: &str, debug: bool)
      -> ErrStr<()> {
   let registry = fetch_token_registry(b).await?;
   let balances = fetch_wallet_balances(b, &registry, addy, debug).await?;
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

    /// Fixed, hardcoded dummy test address -- never read from env.
    const TEST_GLAZEL_ADDY: &'static str =
       "0x6700bD7EAE41434f566e48738813fC585B95669a";
    const AVA: Blockchain = AVALANCHE;

    create_testing!("quiz02::b_gelic");

    run!("gelic_avalanche",
        now(runoff_continuation(&AVA, TEST_GLAZEL_ADDY, true))?
    );

    run!("gelic_binance",
        now(runoff_continuation(&BINANCE, TEST_GLAZEL_ADDY, true))?
    );
}
