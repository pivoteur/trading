use clap::Parser;
use book::{
    cli_utils::generate_banner,
    err_utils::ErrStr,
    file_utils::read_file,
    parse_args_add_banner,
};
use trading::auto_trading::{TokenRegistry, parse_token_registry, wallet_balance};

//----- Token Registry --------------------------------------------------------
const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
const DUST_EPSILON: f64 = 1e-8;

pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}

fn has_balance(balance: f64) -> bool {
    balance > DUST_EPSILON
}

//----- CLI --------------------------------------------------------------------
#[derive(Debug, Parser)]
#[command(name = "gelic")]
#[command(version = "0.1.0")]
struct Args {
    /// The wallet to read. Required -- no env fallback.
    wallet_address: String,
    /// Which chain's data/{blockchain}.toml to load.
    #[arg(long, default_value = "avalanche")]
    blockchain: String,
    /// Balance for just this token; omitted prints every token in the registry.
    #[arg(long)]
    token: Option<String>,
}

//----- Wallet Read --------------------------------------------------------------
pub async fn read_wallet(wallet_address: &str, blockchain: &str, token: Option<&str>) -> ErrStr<()> {
    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry = load_token_registry(&tokens)?;

    println!("wallet {wallet_address} on {blockchain}");

    match token {
        Some(symbol) => {
            let balance = wallet_balance(wallet_address, symbol, &registry).await?;
            println!("  {symbol}: {balance:.8}");
        }
        None => {
            let mut symbols: Vec<&String> = registry.keys().collect();
            symbols.sort();
            for symbol in symbols {
                match wallet_balance(wallet_address, symbol, &registry).await {
                    Ok(balance) if has_balance(balance) => println!("  {symbol}: {balance:.8}"),
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
