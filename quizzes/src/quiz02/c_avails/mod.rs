use chrono::NaiveDate;
use clap::Parser;

use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::{ CsvHeader, CsvWriter },
   err_utils::ErrStr,
   file_utils::lines_from_file
};

use libs::{
   collections::assets::Assets,
   fetchers::{ pivots::parse_pivots, quotes::fetch_quotes },
   types::{
      blockchains::{ Blockchain, Blockchain::AVALANCHE },
      comps::Composition,
      pivots::opens::pivot_assets,
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

   /// open pivots file, e.g.: data/pivots/open/raw/btc-eth.tsv
   open_pivots: String,

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
      runoff_continuation(&pool, &args.addy, chain, &args.open_pivots,
                          date, &quotes, &registry, debug).await
   } else {
      Err(format!("Pivot pool ({pool}) must be in a+b format"))
   }
}

fn compute_available_assets(quotes: &Quotes, blockchain: &Blockchain,
                            pool: &Pool, balances: &Composition,
                            committed: &Assets) -> ErrStr<Composition> {
   let mut available = balances.as_assets();
   committed.assets().iter().for_each(|asset| available.subtract(asset));
   available.update_prices(quotes)?;
   available.as_composition(blockchain, pool, quotes)
}

async fn runoff_continuation(pool: &Pool, addy: &str, blockchain: &Blockchain,
                             path: &str, date: &NaiveDate, quotes: &Quotes, 
                             registry: &TokenRegistry, debug: bool)
      -> ErrStr<()> {
   let bal = fetch_pool_balances(blockchain, quotes, date, pool, addy, 
                                 registry, debug).await?;
   print_composition("Pool balances on wallet", &bal);
   let file = lines_from_file(path)?;
   let ((opens, _closes), _dt) =
      parse_pivots(pool, file, &quotes.aliases, debug)?;
   let committed = pivot_assets(&opens)?;
   let commie = committed.as_composition(blockchain, pool, quotes)?;
   print_composition("Assets committed to pivots", &commie);
   let available =
      compute_available_assets(quotes, blockchain, pool, &bal, &committed)?;
   print_composition("Available assets", &available);
   Ok(())
}

fn print_composition(header: &str, c: &Composition) {
   println!("{header}\n{}\n{}", c.header(), c.as_csv());
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, date_utils::yesterday, utils::now };
   use libs::types::pools::mk_pool;
   use trading::consts::test_wallets::TEST_ADDRESS;

   create_testing!("c_avails");

   run!("avail", {
      let ava = &AVALANCHE;
      let pool = mk_pool("undead", "usdc");
      let yday = &yesterday();
      let quotes = now(fetch_quotes(yday))?;
      let registry = now(fetch_token_registry(ava))?;
      now(runoff_continuation(&pool, TEST_ADDRESS, ava,
                              "data/pivots/open/raw/undead-usdc.tsv",
                              yday, &quotes, &registry, true))?;
   });
}

