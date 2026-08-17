use std::collections::HashMap;
use std::env;
use std::process;
use std::time::Duration;

use chrono::Local;
use serde::Deserialize;

struct ChainInfo {
    api_path: &'static str,
    chain_id: u64,
    display_name: &'static str,
}

fn resolve_chain(input: &str) -> Option<ChainInfo> {
    let info = match input.trim().to_lowercase().as_str() {
        "ethereum" | "eth" | "mainnet" => ChainInfo { api_path: "ethereum", chain_id: 1, display_name: "Ethereum" },
        "avalanche" | "avax" => ChainInfo { api_path: "avalanche", chain_id: 43114, display_name: "Avalanche C-Chain" },
        "polygon" | "matic" => ChainInfo { api_path: "polygon", chain_id: 137, display_name: "Polygon PoS" },
        "arbitrum" | "arb" => ChainInfo { api_path: "arbitrum", chain_id: 42161, display_name: "Arbitrum One" },
        "optimism" | "op" => ChainInfo { api_path: "optimism", chain_id: 10, display_name: "Optimism" },
        "base" => ChainInfo { api_path: "base", chain_id: 8453, display_name: "Base" },
        "bsc" | "bnb" | "binance" => ChainInfo { api_path: "bsc", chain_id: 56, display_name: "BNB Chain" },
        _ => return None,
    };
    Some(info)
}

// (chain api_path, SYMBOL) -> (address, decimals). Placeholder set of majors.
fn token_registry() -> HashMap<(&'static str, &'static str), (&'static str, u8)> {
    let mut m = HashMap::new();
    m.insert(("ethereum", "USDC"), ("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", 6));
    m.insert(("ethereum", "WETH"), ("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", 18));
    m.insert(("ethereum", "WBTC"), ("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", 8));
    m.insert(("avalanche", "USDC"), ("0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E", 6));
    m.insert(("avalanche", "WAVAX"), ("0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7", 18));
    m.insert(("polygon", "USDC"), ("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359", 6));
    m.insert(("polygon", "WMATIC"), ("0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270", 18));
    m.insert(("arbitrum", "USDC"), ("0xaf88d065e77c8cC2239327C5EDb3A432268e5831", 6));
    m.insert(("arbitrum", "WETH"), ("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", 18));
    m.insert(("optimism", "USDC"), ("0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85", 6));
    m.insert(("optimism", "WETH"), ("0x4200000000000000000000000000000000000006", 18));
    m.insert(("base", "USDC"), ("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", 6));
    m.insert(("base", "WETH"), ("0x4200000000000000000000000000000000000006", 18));
    m.insert(("bsc", "USDC"), ("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d", 18));
    m.insert(("bsc", "WBNB"), ("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c", 18));
    m
}

#[derive(Debug)]
pub struct TokenInfo {
    pub symbol: String,
    pub chain_name: String,
    pub chain_id: u64,
    pub address: String,
    pub price_usd: f64,
    pub fetched_at: String,
}

#[derive(Debug, Deserialize)]
struct RoutesResponse {
    code: i64,
    message: String,
    data: Option<RoutesData>,
}

#[derive(Debug, Deserialize)]
struct RoutesData {
    #[serde(rename = "routeSummary")]
    route_summary: RouteSummary,
}

#[derive(Debug, Deserialize)]
struct RouteSummary {
    #[serde(rename = "amountInUsd")]
    amount_in_usd: String,
}

pub async fn get_token_info(
    client: &reqwest::Client,
    chain: &str,
    symbol: &str,
) -> Result<TokenInfo, Box<dyn std::error::Error>> {
    let chain_info = resolve_chain(chain)
        .ok_or_else(|| format!("'{chain}' isn't a chain this prototype knows about"))?;

    let symbol_upper = symbol.trim().to_uppercase();
    let registry = token_registry();

    let (token_address, decimals) = registry
        .get(&(chain_info.api_path, symbol_upper.as_str()))
        .copied()
        .ok_or_else(|| format!("'{symbol_upper}' on {} isn't in the registry yet", chain_info.display_name))?;

    // Reference token to price against; fall back off USDC if that's what we're pricing.
    let (ref_address, _) = if symbol_upper == "USDC" {
        registry
            .iter()
            .find(|((c, sym), _)| *c == chain_info.api_path && *sym != "USDC")
            .map(|(_, v)| *v)
            .ok_or("no non-USDC reference token available on this chain")?
    } else {
        *registry.get(&(chain_info.api_path, "USDC")).unwrap()
    };

    let amount_in = 10u128.pow(decimals as u32);
    let url = format!("https://aggregator-api.kyberswap.com/{}/api/v1/routes", chain_info.api_path);
    let client_id = env::var("KYBERSWAP_CLIENT_ID").unwrap_or_else(|_| "frignan-prototype".to_string());

    let resp: reqwest::Response = client
        .get(&url)
        .header("x-client-id", client_id)
        .query(&[("tokenIn", token_address.to_string()), ("tokenOut", ref_address.to_string()), ("amountIn", amount_in.to_string())])
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(format!("KyberSwap returned HTTP {status}: {body}").into());
    }

    let parsed: RoutesResponse = serde_json::from_str(&body)
        .map_err(|e| format!("couldn't parse KyberSwap response ({e}): {body}"))?;
    if parsed.code != 0 && parsed.code != 200 {
        return Err(format!("KyberSwap error {}: {}", parsed.code, parsed.message).into());
    }

    let data = parsed.data.ok_or_else(|| format!("KyberSwap response had no route data: {body}"))?;
    let price_usd: f64 = data.route_summary.amount_in_usd.parse()
        .map_err(|e| format!("couldn't parse amountInUsd '{}' as a number: {e}", data.route_summary.amount_in_usd))?;

    Ok(TokenInfo {
        symbol: symbol_upper,
        chain_name: chain_info.display_name.to_string(),
        chain_id: chain_info.chain_id,
        address: token_address.to_string(),
        price_usd,
        fetched_at: Local::now().format("%Y-%m-%d %H:%M:%S %:z").to_string(),
    })
}

fn print_usage_and_exit(program: &str) -> ! {
    eprintln!("usage: {program} <blockchain> <TOKEN_SYMBOL>");
    eprintln!("example: {program} avalanche WAVAX");
    process::exit(1);
}
// ----- fn main -------------------------------------------------------------
#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args.first().map(|s| s.as_str()).unwrap_or("frignan");
    if args.len() != 3 {
        print_usage_and_exit(program);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_else(|e| { eprintln!("failed to build HTTP client: {e}"); process::exit(1); });

    match get_token_info(&client, &args[1], &args[2]).await {
        Ok(info) => {
            println!("============================================");
            println!(" {}  ({})", info.symbol, info.chain_name);
            println!("============================================");
            println!("Chain:         {} (chain id {})", info.chain_name, info.chain_id);
            println!("Token address: {}", info.address);
            println!("Price (USD):   ${:.8}", info.price_usd);
            println!("Fetched at:    {}", info.fetched_at);
            println!("============================================");
        }
        Err(e) => {
            eprintln!("frignan: {e}");
            process::exit(1);
        }
    }
}
