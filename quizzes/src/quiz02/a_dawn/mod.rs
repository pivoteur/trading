use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{ fs::{OpenOptions, File}, path::Path };
use chrono::{ NaiveDate, NaiveDateTime };
use uuid::Uuid;

use book::{
   debug,
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::{ parse_items, as_csv },
   date_utils::today;
   err_utils::{ ErrStr, err_or },
   file_utils::read_file,
   string_utils::{ UppercaseString, s }
};

use libs::{
   types::{
      blockchains::{ Blockchain, Blockchain::AVALANCHE },
      pools::Pool,
      wallets::Wallet
   },
   wallets::{ factory::connect_wallet, mock::mock_connection }
};

use trading::{
   types::modes::execution::{ Execution, Execution::LIVE },
   wallets::factory::connect_wallet
};

#[derive(Parser, Debug)]
#[command(name = "dawn")]
#[command(version = "0.0.1")]
struct Cli {
   /// primary token
    primary: UppercaseString,

   /// token pivoted on
    pivot: UppercaseString,

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

   /// date to run open pivots
   #[arg(long, env = "LE_DATE")]
   date: NaiveDate,

   /// dry-run or live
    #[arg(long, default_value_t = false)] dry_run: bool,

   /// show debugging information
   #[args(short, long, default_value_t = false)] debug: bool
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenPivotRow {
    id: String,
    wallet_address: String,
    network: Blockchain,
    pair: Pool,
    token_in: UppercaseString,
    token_out: UppercaseString,
    amount_in: f32,
    amount_out: f32,
    timestamp: NaiveDateTime,
    closed_on: String,
    tx_id_open: String,
    tx_id_close: String
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

pub async fn runoff_with_args() -> ErrStr<()> {
   let args = parse_args_add_banner!(Cli);
   let mode = Execution::from(args.dry_run);
   let blockchain = &args,blockchain;
   let wallet =
      connect_wallet(blockchain, &args.wallet, &args.keystore_path,
                     args.debug, &mode).await?;
   runoff_continuation(&wallet, &args.blockchain, &args.primary,
                       args.amount, &args.pivot, &args.date, args.debug).await
}

async runoff_continuation(wallet: &Box<dyn Wallet>, blockchain: &Blockchain,
                          primary: &str, amount: f32, pivot: &str,
                          date: &NaiveDate, debug: bool) -> ErrStr<()> {
    debug!("runoff_continuation", debug);
    let balances = wallet.balances(date).await?;
    let pool = balances.pool(blockchain, primary, pivot, debug)?;
    let open_path = format!("data/open-pivots-{}.csv", pool.file_name());
    let file = read_file(&open_path)?;
    let mut pivots = parse_items::<OpenPivotRow>(&file)?;

    // 3 scenarii, in order:
    // 1. virtual pivot (let's make the rules for that)
    // 2. actual pivot (we have enough of primary)
    // 3. pass (no resources available)
    
    let locked: f32 =
       open_pivots.iter()
                  .filter(|p| &p.token_out == primary && &p.closed_on == "PENDING")
            .map(|p| p.amount_out).sum();
    let should_execute_on_chain = primary == "UNDEAD"
                               || 
    if should_execute_on_chain {
        let available_funds =
           balances.get(&primary).unwrap_or(&0.0) - locked;
        if available_funds >= amount {
            log!("🎯 Rule match: UNDEAD input. Executing on-chain trade...");
            if let EXECUTED { tx_hash, actual_received } =
               wallet.trade(primary, amount, pivot, None, 0).await? {
               open_pivots.push(OpenPivotRow {
                   id: Uuid::new_v4().to_string(),
                   wallet_address: wallet.wallet_address(),
                   network: blockchain,
                   pair: pool,
                   token_in: primary,
                   token_out: pivot,
                   amount_in: amount,
                   amount_out: actual_received,
                   timestamp: Utc::now().timestamp(),
                   closed_on: s("PENDING"),
                   tx_id_open: tx_hash,
                   tx_id_close: s("PENDING")
               });
            }
        } else {
           log!("⚠️ [DEBUG REMARK] Insufficient physical UNDEAD balance.");
        }
    } else {
        let locked: f32 = open_pivots.iter()
            .filter(|p| &p.token_out == primary && &p.closed_on == "PENDING")
            .map(|p| p.amount_out).sum();
        let available_funds = balances.get(&primary).unwrap_or(&0.0) - locked;
        if available_funds >= amount {
            log!("🔮 Rule fallback: attempting a Virtual Pivot (Locking {})...", primary);
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
