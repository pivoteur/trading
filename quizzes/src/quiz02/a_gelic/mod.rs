use clap::Parser;
use serde::Serialize;
use serde_with::{ serde_as, DisplayFromStr };

use book::{
    debug,
    parse_args_add_banner,
    cli_utils::generate_banner,
    currency::usd::mk_usd,
    cvs_utils::as_csv,
    err_utils::ErrStr,
    file_utils::read_file,
    string_utils::s
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   fetchers::tokens::fetch_tokens,
   types::tokens::TokenRegistry,
   wallets::wallet_balance
};

const DUST_EPSILON: f64 = 1e-8;

fn has_balance(balance: f64) -> bool {
    balance > DUST_EPSILON
}

//----- CLI -------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "1.0.0")]
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
   let nav = mk_usd(quote.amount * amount);
   TokenBalance { token: s(tok), quote, amount, nav }
}

//----- Wallet Read ---------------------------------------------

async fn read_wallet(wallet_address: &str, blockchain: &Blockchain,
                         debug: bool) -> ErrStr<Vec<TokenBalance>> {
    debug!("read_wallet", debug);
    let registry = fetch_tokens(&blockchain)?;

    log!("wallet {wallet_address} on {blockchain}");

    let mut symbols: Vec<String> = registry.keys().collect();
    symbols.sort();
    let mut ans = Vec::new();
    for symbol in symbols {
       match wallet_balance(wallet_address, symbol, &registry).await {
          Ok(balance) if has_balance(balance) => { 
             log!("{symbol}: {balance:.8}"),
             let qt = query_quote(
                    Ok(_) => {} // zero/dust balance -- not actually in the wallet, skip it
                    Err(e) => println!("  {symbol}: ! could not read balance ({e})"),
                }
            }
        }
    }

    Ok(())
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    read_wallet(&args.wallet_address, &args.blockchain, args.token.as_deref()).await
}

//----- UNIT TESTS -------------------------------------------------------------
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_load_token_registry_has_btc_undead_avax() -> ErrStr<()> {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        for symbol in ["BTC", "UNDEAD", "AVAX"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in avalanche.toml");
        }
        Ok(())
    }

    #[test]
    fn test_wallet_address_required() {
        let result = Args::try_parse_from(["gelic"]);
        assert!(result.is_err(), "wallet_address has no default and no env fallback -- omitting it must fail to parse");
    }

    #[test]
    fn test_blockchain_default() {
        let args = Args::try_parse_from(["gelic", "0x123"]).expect("should parse with just the wallet address");
        assert_eq!(args.blockchain, "avalanche");
    }

    #[test]
    fn test_blockchain_override() {
        let args = Args::try_parse_from(["gelic", "0x123", "--blockchain", "binance"]).expect("should parse with --blockchain given");
        assert_eq!(args.blockchain, "binance");
    }

    #[test]
    fn test_token_default() {
        let args = Args::try_parse_from(["gelic", "0x123"]).expect("should parse with just the wallet address");
        assert_eq!(args.token, None);
    }

    #[test]
    fn test_token_override() {
        let args = Args::try_parse_from(["gelic", "0x123", "--token", "BTC"]).expect("should parse with --token given");
        assert_eq!(args.token.as_deref(), Some("BTC"));
    }

    #[test]
    fn test_has_balance() {
        assert!(!has_balance(0.0));
        assert!(!has_balance(1e-9), "sub-epsilon dust must not count as a real balance");
        assert!(has_balance(0.00000434), "a real, if small, balance must still show");
    }
}

//----- FUNCTIONAL TESTS -------------------------------------------------------
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{create_testing, utils::now};

    /// Fixed, hardcoded dummy test address -- never read from env.
    const TEST_GLAZEL_ADDRESS: &str = "0x6700bD7EAE41434f566e48738813fC585B95669a";

    create_testing!("quiz02::a_gelic");

    run!("gelic_functionality", {
        now(read_wallet(TEST_GLAZEL_ADDRESS, "avalanche", None))?;
        println!("gelic is ok");
    });

    run!("read_wallet_single_token", {
        now(read_wallet(TEST_GLAZEL_ADDRESS, "avalanche", Some("BTC")))?;
    });
}
