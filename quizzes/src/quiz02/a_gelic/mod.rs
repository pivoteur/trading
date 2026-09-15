use clap::Parser;
use serde::Serialize;
use serde_with::{ serde_as, DisplayFromStr };

use book::{
    debug,
    parse_args_add_banner,
    cli_utils::generate_banner,
    currency::usd::{ USD, mk_usd },
    csv_utils::as_csv,
    err_utils::ErrStr,
    string_utils::s
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   auto_trading::query_quote,
   fetchers::{ tokens::fetch_tokens, wallets::fetch_wallet_balance }
};

const DUST_EPSILON: f64 = 1e-8;

fn has_balance(balance: f64) -> bool {
    balance > DUST_EPSILON
}

//----- CLI -------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "1.1.0")]
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

// ----- TokenBalance -------------------------------------------------------

#[serde_as]
#[derive(Debug, Clone, Serialize)]
struct TokenBalance {
   token: String,
   #[serde_as(as = "DisplayFromStr")]
   quote: USD,
   amount: f32,
   #[serde_as(as = "DisplayFromStr")]
   nav: USD
}

fn mk_token_balance(tok: &str, quote: USD, amount: f32) -> TokenBalance {
   let nav = mk_usd(quote.amount() * amount);
   TokenBalance { token: s(tok), quote, amount, nav }
}

//----- Wallet Read ---------------------------------------------

async fn read_wallet(wallet_address: &str, blockchain: &Blockchain,
                         debug: bool) -> ErrStr<Vec<TokenBalance>> {
    debug!("read_wallet", debug);
    let registry = fetch_tokens(&blockchain)?;

    log!("wallet {} on {}", wallet_address, blockchain);

    let mut symbols: Vec<String> = registry.keys().collect();
    symbols.sort();
    let mut ans = Vec::new();
    for symbol in symbols {
       match fetch_wallet_balance(wallet_address, symbol, &registry).await {
          Ok(balance) if has_balance(balance) => { 
             log!("Token {}: {:.8}", symbol, balance);
             let qt = query_quote(blockchain, &registry, symbol, debug).await?;
             let bal = mk_token_balance(symbol, qt, balance);
             ans.push(bal);
          },
          Ok(_) => {
             log!("Token {} zero/dust balance -- not in the wallet, skip it",
                  symbol);
          },
          Err(e) => log!("Token {}: ! could not read balance ({})", symbol, e)
       }
    }
    Ok(ans)
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    let balances =
       read_wallet(&args.wallet_address, &args.blockchain, args.debug).await?;
    println!("{}", as_csv(&balances, true)?);
}

//----- UNIT TESTS -------------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod unit_tests {
    use super::*;

    #[test]
    fn test_has_balance() {
        assert!(!has_balance(0.0));
        assert!(!has_balance(1e-9),
            "sub-epsilon dust must not count as a real balance");
        assert!(has_balance(0.00000434),
             "a real, if small, balance must still show");
    }
}

//----- FUNCTIONAL TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{
        create_testing,
        types::blockchains::Blockchain::BINANCE,
        utils::now
    };

    /// Fixed, hardcoded dummy test address -- never read from env.
    const TEST_GLAZEL_ADDRESS: &str =
       "0x6700bD7EAE41434f566e48738813fC585B95669a";

    create_testing!("quiz02::a_gelic");

    run!("read_wallet_avalanche", {
        let bal = now(read_wallet(TEST_GLAZEL_ADDRESS, AVALANCHE, true))?;
        println!("{}", as_csv(&bal, true)?);
    });

    run!("read_wallet_binance", {
        let bal = now(read_wallet(TEST_GLAZEL_ADDRESS, BINANCE, true))?;
        println!("{}", as_csv(&bal, true)?);
    });
}
