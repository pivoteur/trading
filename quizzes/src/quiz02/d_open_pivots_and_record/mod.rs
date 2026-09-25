use std::pin::Pin;
use chrono::NaiveDate;
use clap::Parser;

use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::{ CsvHeader, CsvWriter },
   err_utils::ErrStr,
   file_utils::lines_from_file,
   list_utils::async_filter_map,
   num::floats::comma_floats::CommaFloat,
   string_utils::s
};

use libs::{
   fetchers::pivots::parse_pivots,
   types::{
      blockchains::{ Blockchain, Blockchain::AVALANCHE },
      comps::{ Composition, from_assets },
      pivots::opens::pivot_assets,
      pools::{ Pool, compute_pool },
      quotes::mk_quotes,
      tokens::coins::{ Coin, mk_coin }
   }
};

use trading::{
   auto_trading::query_quote,
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

   /// Amount of primary asset to pivot (e.g., if BTC, then 0.01, say)
   primary_amt: CommaFloat,

   /// Amount of pivot asset to pivot (e.g., if ETH, then 0.34, say)
   pivot_amt: CommaFloat,

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

fn compute_coin<'a>(chain: &'a Blockchain, date: &'a NaiveDate,
                          registry: &'a TokenRegistry, debug: bool)
      -> impl Fn((String, f32))
      -> Pin<Box<dyn Future<Output = ErrStr<(Coin, (String, f32))>> + 'a>> {
   move | (token, max_amt): (String, f32) | Box::pin(async move {
      let quote = query_quote(chain, registry, &token, debug).await?;
      let coin =
         mk_coin(&(chain.clone(), token.clone()), max_amt, &quote, date);
      Ok((coin, (token, quote.amount())))
   })
}

pub async fn runoff_with_args() -> ErrStr<()> {
   let args = parse_args_add_banner!(Args);
   let pool = &args.pool;
   let tokens: Vec<&str> = pool.split("+").collect();
   if let [primary, pivot] = tokens.as_slice() {
      let date = &args.date;
      let chain = &args.blockchain;
      let debug = args.debug;
      let registry = fetch_token_registry(chain).await?;
      runoff_continuation(&[(s(primary), args.primary_amt.into()),
                            (s(pivot), args.pivot_amt.into())],
                          &args.addy, chain, &args.open_pivots, date, &registry,
                          debug).await
   } else {
      Err(format!("Pivot pool ({pool}) must be in a+b format"))
   }
}

async fn runoff_continuation(amounts: &[(String, f32)], addy: &str, 
                             blockchain: &Blockchain, path: &str,
                             date: &NaiveDate, registry: &TokenRegistry,
                             debug: bool) -> ErrStr<()> {
   let (primary, _prim_amt) = amounts.first().unwrap();
   let (pivot, _piv_amt) = amounts.last().unwrap();
   let coins_n_prices =
      async_filter_map(compute_coin(blockchain, date, registry, debug),
                       amounts.to_vec()).await?;

   // unzip does not work here as unzip requires Default-implementation

   if let [(coin1, price1), (coin2, price2)] = coins_n_prices.as_slice() {
      let coins = &[coin1.clone(), coin2.clone()];
      let prices = &[price1, price2];
      let aliased_prices: Vec<(&str, f32)> =
         prices.into_iter().map(|(a,b)| (a.as_str(), *b)).collect();
      let quotes = mk_quotes(date, &aliased_prices);
      let pool = compute_pool(&quotes, primary, pivot, debug)?;
      let pc = print_composition(&pool);
      let bal = fetch_pool_balances(blockchain, &quotes, date, &pool, addy, 
                                    registry, debug).await?;
      pc("Pool balances on wallet", bal.clone());
      let file = lines_from_file(path)?;
      let ((opens, _closes), _dt) =
         parse_pivots(&pool, file, &quotes.aliases, debug)?;
      let committed = pivot_assets(&opens)?;
      let commie = committed.as_composition(blockchain, &pool, &quotes)?;
      pc("Assets committed to pivots", commie);
      let available =
         bal.compute_available_assets(&quotes, blockchain, &pool, &committed)?;
      pc("Available assets", available);
      let hard_top = from_assets(coins, debug)?;
      pc("Targeted pivot amounts", hard_top);
      Ok(())
   } else {
      Err(s("Amounts not a pair of token-prices"))
   }
}

fn print_composition<'a>(pool: &'a Pool) -> impl Fn(&'a str, Composition) {
   move | header: &'a str, c: Composition |
      println!("{pool} {header}\n{}\n{}", c.header(), c.as_csv())
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, date_utils::yesterday, utils::now };
   use trading::consts::{ UNDEAD, test_wallets::TEST_ADDRESS };

   create_testing!("c_avails");

   run!("avail", {
      let ava = &AVALANCHE;
      let yday = &yesterday();
      let registry = now(fetch_token_registry(ava))?;
      now(runoff_continuation(&[(s(UNDEAD), 10000.0), (s("USDC"), 10.0)],
                              TEST_ADDRESS, ava,
                              "data/pivots/open/raw/undead-usdc.tsv",
                              yday, &registry, true))?;
   });
}
