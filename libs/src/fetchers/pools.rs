use std::pin::Pin;
use chrono::NaiveDate;

use book::{
   currency::usd::mk_usd,
   err_utils::ErrStr,
   list_utils::async_filter_map
};
use libs::types::{
   comps::{ Composition, mk_composition },
   blockchains::Blockchain,
   pools::Pool,
   quotes::Quotes,
   tokens::coins::{ Coin, mk_coin }
};
use super::wallets::fetch_token_balance;
use crate::types::tokens::TokenRegistry;

pub async fn fetch_pool_balances(blockchain: &Blockchain, quotes: &Quotes,
                                 date: &NaiveDate, pool: &Pool, addy: &str,
                                 registry: &TokenRegistry, debug: bool)
      -> ErrStr<Composition> {
   let coins = 
      async_filter_map(coin(blockchain, quotes, date, addy, registry, debug),
                       pool.as_vec()).await?;
   if let [prim, piv] = coins.as_slice() {
      Ok(mk_composition(prim, piv))
   } else {
      Err(format!("not two assets in {coins:?}"))
   }
}

fn coin<'a>(b: &'a Blockchain, q: &'a Quotes, d: &'a NaiveDate, addy: &'a str,
            r: &'a TokenRegistry, debug: bool)
      -> impl Fn(String)
      -> Pin<Box<dyn Future<Output=ErrStr<Coin>> + 'a>> {
   move |token: String| {
      Box::pin(async move {
         let bal0 = fetch_token_balance(b, addy, &token, r, debug).await?;
         let bal = bal0.unwrap_or(0.0);
         let qt = q.lookup(&token)?;
         Ok(mk_coin(&(b.blockchain(), token), bal, &mk_usd(qt), d))
      })
   }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{
      create_testing,
      csv_utils::{ CsvWriter, CsvHeader },
      date_utils::yesterday,
      utils::now
   };
   use libs::{
      fetchers::quotes::fetch_quotes,
      types::{ blockchains::Blockchain::AVALANCHE, pools::compute_pool }
   };
   use crate::{
      consts::test_wallets::TEST_ADDRESS,
      fetchers::tokens::fetch_token_registry
   };

   create_testing!("fetchers::pools");

   run!("fetch_pool_balances", {
      let ava = &AVALANCHE;
      let reg = now(fetch_token_registry(ava))?;
      let yday = &yesterday();
      let qt = now(fetch_quotes(yday))?;
      let pool = compute_pool(&qt, "usdc", "undead", true)?;
      let usdc_undead =
         now(fetch_pool_balances(ava, &qt, yday, &pool, TEST_ADDRESS, &reg,
                                true))?;
         println!("{pool} pivot pool:

{}
{}", usdc_undead.header(), usdc_undead.as_csv());
   });
}
