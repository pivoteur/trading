use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{ fs::{OpenOptions, File}, path::Path };
use chrono::NaiveDate;
use uuid::Uuid;

use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::{ parse_items, as_csv },
   err_utils::{ ErrStr, err_or },
   file_utils::read_file
};

use libs::{
   types::{
      blockchains::{ Blockchain, Blockchain::AVALANCHE },
      wallets::Wallet
   },
   wallets::{ factory::connect_wallet, mock::mock_connection }
};

use trading::types::modes::execution::{ Execution, Execution::LIVE };

#[derive(Parser, Debug)]
#[command(name = "dawn")]
#[command(version = "0.0.1")]
struct Cli {
   /// primary token
    primary: String,
   /// token pivoted on
    pivot: String,
   /// amount of primary to pivot
    amount: f32,
   /// pivot pool wallet address
    #[arg(long)] wallet: String,
   /// blockchain of pivot pool
    #[arg(long, default_value_t = AVALANCHE)] blockchain: Blockchain,
   /// keystore authorizing trades
    #[arg(long)] keystore_path: String,
   /// minimum returned from open-pivot trade
    #[arg(long, default_value_t = 0.0)] floor: f32,
   /// dry-run or live
    #[arg(long, default_value_t = false)] dry_run: bool,
   /// show debugging information
   #[args(short, long, default_value_t = false)] debug: bool
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenPivotRow {
    id: String, wallet_address: String, network: String, pair: String,
    token_in: String, token_out: String, amount_in: f32, amount_out: f32,
    timestamp: i64, closed_on: String, tx_id_open: String, tx_id_close: String,
}

fn write_csv_table<T: Serialize>(path: &str, records: &[T]) -> ErrStr<()> {
    let file = err_or(OpenOptions::new().write(true)
                                 .create(true)
                                 .truncate(true)
                                 .open(path),
                      &format!("Could not open file {path}"))?;
    let mut wtr = csv::Writer::from_writer(file);
    for (i, record) in records.into_iter().enumerate() {
       let x = i +1;
       err_or(wtr.serialize(record),
              &format!("Could not serialize record {x}"))?;
    }
    err_or(wtr.flush(), "Could not flush file")?;
    Ok(())
}

#[tokio::main]
async fn main() -> ErrStr<()> {
   let args = parse_args_add_banner!(Cli);
   let mode = Execution::from(args.dry_run;
   let blockchain = &args,blockchain;
   let wallet_client = if mode == DRYRUN {
      mock_connection(blockchain, true).await?
   } else {
      connect_wallet(args.blockchain, &args.wallet, &args.keystore_path,
                     false).await?
   };

   let sanitized_pair = args.pair.replace('+', "-").to_lowercase();
    let open_path = format!("data/open-pivots-{}.csv", sanitized_pair);
    let file = read_file(&open_path)?;
    let mut open_pivots = parse_items::<OpenPivotRow>(&file)?;
xxx
    let today = Local::now().naive_local().date();
    let should_execute_on_chain = args.token_in.to_uppercase() == "UNDEAD";

    if should_execute_on_chain {
        let locked: f32 = open_pivots.iter()
            .filter(|p| p.token_in == args.token_in && p.closed_on == "PENDING")
            .map(|p| p.amount_in).sum();
        
        let balances = wallet_client.balances(&today).await.map_err(|e| format!("{:?}", e))?;
        let available_funds = balances.get(&args.token_in).unwrap_or(&0.0) - locked;

        if available_funds >= args.amount_in {
            println!("🎯 Rule match: UNDEAD input. Executing on-chain trade...");
            wallet_client.trade().await.map_err(|e| format!("{:?}", e))?;
            
            open_pivots.push(OpenPivotRow {
                id: Uuid::new_v4().to_string(), wallet_address: args.wallet, network: args.network, pair: args.pair,
                token_in: args.token_in, token_out: args.token_out, amount_in: args.amount_in, amount_out: args.amount_out,
                timestamp: Utc::now().timestamp(), closed_on: "PENDING".to_string(),
                tx_id_open: "0xRealOpenTxHash".to_string(), tx_id_close: "PENDING".to_string()
            });
        } else { println!("⚠️ [DEBUG REMARK] Insufficient physical UNDEAD balance."); }
    } else {
        let locked: f32 = open_pivots.iter()
            .filter(|p| p.token_out == args.token_out && p.closed_on == "PENDING")
            .map(|p| p.amount_out).sum();
        
        let balances = wallet_client.balances(&today).await.map_err(|e| format!("{:?}", e))?;
        let available_funds = balances.get(&args.token_out).unwrap_or(&0.0) - locked;

        if available_funds >= args.amount_out {
            println!("🔮 Rule fallback: Standardizing on a Virtual Pivot (Locking {})...", args.token_out);
            open_pivots.push(OpenPivotRow {
                id: Uuid::new_v4().to_string(), wallet_address: args.wallet, network: args.network, pair: args.pair,
                token_in: args.token_in, token_out: args.token_out, amount_in: args.amount_in, amount_out: args.amount_out,
                timestamp: Utc::now().timestamp(), closed_on: "PENDING".to_string(),
                tx_id_open: "virtual_pivot".to_string(), tx_id_close: "PENDING".to_string()
            });
        } else { println!("⚠️ [DEBUG REMARK] Insufficient token backing for virtual pivot spacing."); }
    }

    write_csv_table(&open_path, &open_pivots)?;
    Ok(())
}
