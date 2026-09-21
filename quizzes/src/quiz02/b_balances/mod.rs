use chrono::NaiveDate;
use clap::Parser;
use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::{ CsvHeader, CsvWriter },
   err_utils::ErrStr
};
use libs::{
   fetchers::quotes::fetch_quotes,
   types::{
      blockchains::{ Blockchain, Blockchain::AVALANCHE },
      pools::{ Pool, compute_pool },
      quotes::Quotes
   }
};
use trading::{
   fetchers::{
      pools::fetch_pool_balances,
      tokens::fetch_token_registry
   },
   types::tokens::TokenRegistry
};

#[derive(Debug, Parser)]
#[command(name="balancer")]
#[command(version="0.1.1")]
struct Args {

   /// Pivot pool, e.g. BTC+ETH
   pool: String,

   /// wallet address, e.g. 0x1234...
   addy: String,

   /// Blockchain on which the pool resides
   #[arg(long, default_value_t=AVALANCHE)]
   blockchain: Blockchain,

   /// Date for analysis (usually today)
   #[arg(long, env="LE_DATE")]
   date: NaiveDate,

   /// Show debugging information
   #[arg(short, long)]
   debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
   let args = parse_args_add_banner!(Args);
   let pool = &args.pool;
   let tokens: Vec<&str> = pool.split("+").collect();
   if let [primary, pivot] = tokens.as_slice() {
      let date = &args.date;
      let chain = &args.blockchain;
      let quotes = fetch_quotes(date).await?;
      let debug = args.debug;
      let pool = compute_pool(&quotes, primary, pivot, debug)?;
      let registry = fetch_token_registry(chain).await?;
      runoff_continuation(&pool, &args.addy, chain, date, &quotes, &registry,
                          debug).await
   } else {
      Err(format!("Pivot pool ({pool}) must be in a+b format"))
   }
}

async fn runoff_continuation(pool: &Pool, addy: &str, blockchain: &Blockchain,
                             date: &NaiveDate, quotes: &Quotes, 
                             registry: &TokenRegistry, debug: bool)
      -> ErrStr<()> {
   let bal = fetch_pool_balances(blockchain, quotes, date, pool, addy, 
                                 registry, debug).await?;
   println!("{}\n{}", bal.header(), bal.as_csv());
   Ok(())
}

// ----- TEST -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, date_utils::yesterday, utils::now };
   use libs::types::pools::mk_pool;
   use trading::consts::test_wallets::TEST_ADDRESS;

   create_testing!("quiz02::b_balances");

   run!("balancer", {
      let yday = &yesterday();
      let quotes = now(fetch_quotes(&yday))?;
      let chain = &AVALANCHE;
      let pool = mk_pool("UNDEAD", "USDC");
      let registry = now(fetch_token_registry(chain))?;
      now(runoff_continuation(&pool, TEST_ADDRESS, chain, yday, &quotes,
                              &registry, true))?;
   });
}
