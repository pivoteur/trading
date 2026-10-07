use chrono::NaiveDate;

use book::err_utils::ErrStr;
use libs::types::{ comps::Composition, pools::Pool, quotes::Quotes };
use crate::types::wallets::Wallet;

pub async fn fetch_pool_balances(wallet: &Box<dyn Wallet>, quotes: &Quotes,
                                 date: &NaiveDate, pool: &Pool)
      -> ErrStr<Composition> {
   let balances = wallet.balances(date).await?;
   balances.as_composition(wallet.blockchain(), pool, quotes)
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{
      create_testing,
      csv_utils::list_csv,
      date_utils::yesterday,
      utils::now
   };
   use libs::{
      fetchers::quotes::fetch_quotes,
      types::{ blockchains::Blockchain::AVALANCHE, pools::compute_pool }
   };
   use crate::wallets::mock::mock_connection;

   create_testing!("fetchers::pools");

   run!("fetch_pool_balances", {
      let ava = &AVALANCHE;
      let yday = &yesterday();
      let qt = now(fetch_quotes(yday))?;
      let pool = compute_pool(&qt, "btc", "eth", true)?;
      let wallet = mock_connection(ava, true);
      println!("Assets on (mock) wallet:\n\n{}",
               list_csv(&[now(wallet.balances(yday))?], true));
      let btc_eth = now(fetch_pool_balances(&wallet, &qt, yday, &pool))?;
      println!("{pool} pivot pool:\n\n{}", list_csv(&[btc_eth], true))
   });
}
