use clap::Parser;
use book::{
    cli_utils::generate_banner,
    err_utils::ErrStr,
    file_utils::read_file,
    parse_args_add_banner,
    utils::get_env,
};
use trading::auto_trading::{
    TokenRegistry,
    parse_token_registry,
    wallet_balance,
    replay_log,
    balance_snapshot,
    committed_amount,
    UNDEAD,
};


const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
const BTC: &str = "BTC";

#[derive(Debug, Parser)]
#[command(name = "gemynd")]
#[command(version = "0.1.0")]
struct Args {
    /// Which auto-trader to report on, e.g. `meta tva`. Reads
    /// {NAME}_WALLET_ADDRESS (required) and {NAME}_TRADE_LOG (optional --
    /// a filename in data/, only set for traders that keep a pivot log)
    /// from the environment.
    name: String,
    #[arg(long, default_value = "avalanche")]
    blockchain: String,
    #[arg(short = 'd', long, default_value_t = false)]
    debug: bool,
}

pub async fn run_report(name: &str, blockchain: &str, debug: bool) -> ErrStr<()> {
    let prefix = name.to_uppercase();
    let wallet_address = get_env(&format!("{prefix}_WALLET_ADDRESS"))?;
    let trade_log = std::env::var(format!("{prefix}_TRADE_LOG")).ok();

    if debug {
        eprintln!("[{name}] wallet {wallet_address}, log {trade_log:?}");
    }

    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry: TokenRegistry = parse_token_registry(&tokens)?;

    println!("=== {name} ===");
    println!("wallet            {wallet_address}");

    if let Some(log_file) = trade_log {
        let log_path = format!("{DATA_DIR}/{log_file}");
        let (open_pivots, _next_pivot_id, _next_close_id, stats) = replay_log(&log_path)?;
        let committed_btc = committed_amount(BTC, &open_pivots);
        let committed_undead = committed_amount(UNDEAD, &open_pivots);
        let snap = balance_snapshot(&wallet_address, &registry, BTC, committed_btc, committed_undead).await?;

        println!("BTC balance       {:.4}  (committed {:.4}, available {:.4})", snap.asset_balance, snap.asset_committed, snap.asset_available);
        println!("UNDEAD balance    {:.8}  (committed {:.8}, available {:.8})", snap.undead_balance, snap.undead_committed, snap.undead_available);
        println!("open pivots       {}", open_pivots.len());
        println!("opens / closes    {} / {}", stats.total_opens, stats.total_closes);
        println!("realized gain     BTC {:+.4}   UNDEAD {:+.8}", stats.total_gain_asset, stats.total_gain_undead);
        println!("gas spent         {:.5} AVAX", stats.total_gas_avax);
        println!("avg roi / apr     {:.2}% / {:.2}%", stats.avg_roi() * 100.0, stats.avg_apr() * 100.0);
    } else {
        let (btc, undead) = tokio::try_join!(
            wallet_balance(&wallet_address, BTC, &registry),
            wallet_balance(&wallet_address, UNDEAD, &registry),
        )?;
        println!("BTC balance       {btc:.4}");
        println!("UNDEAD balance    {undead:.8}");
    }

    Ok(())
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    run_report(&args.name, &args.blockchain, args.debug).await
}
