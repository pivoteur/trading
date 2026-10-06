use chrono::NaiveDate;

use book::err_utils::ErrStr;
use libs::{
   collections::assets::mk_assets,
   types::{ comps::Composition, pools::Pool, quotes::Quotes }
};
use crate::types::wallets::Wallet;

pub async fn fetch_pool_balances<W: Wallet>(wallet: &W, quotes: &Quotes,
                                            date: &NaiveDate, pool: &Pool)
      -> ErrStr<Composition> {
   let balances = wallet.balances().await?;
   let mut assets = mk_assets();
   let chain = &wallet.blockchain();
   balances.iter()
           .for_each(|balance| assets.add(balance.as_coin(chain, date)));
   assets.as_composition(chain, pool, quotes)
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{
      create_testing,
      csv_utils::{ as_csv, list_csv },
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
               as_csv(&now(wallet.balances())?, true)?);
      let btc_eth = now(fetch_pool_balances(&wallet, &qt, yday, &pool))?;
      println!("{pool} pivot pool:\n\n{}", list_csv(&[btc_eth], true))
   });
}
