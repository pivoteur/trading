use book::err_utils::ErrStr;
use libs::types::blockchains::Blockchain;
use super::wallets::fetch_token_balance;
use crate::{
   consts::UNDEAD,
   types::{
      balances::pools::{ BalanceSnapshot, mk_balance_snapshot },
      tokens::TokenRegistry
   }
};

pub async fn fetch_undead_pool_snapshot(blockchain: &Blockchain, addy: &str,
                                        registry: &TokenRegistry, prim: &str,
                                        committed: f32, undead_committed: f32,
                                        debug: bool)
      -> ErrStr<BalanceSnapshot> {
    let asset_balance =
       fetch_token_balance(blockchain, addy, prim, registry, debug).await?;
    let undead_balance =
       fetch_token_balance(blockchain, addy, UNDEAD, registry, debug).await?;
    Ok(mk_balance_snapshot(prim, asset_balance, committed,
                           undead_balance, undead_committed))
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };
   use libs::types::blockchains::Blockchain::AVALANCHE;
   use crate::{
      consts::test_wallets::TEST_ADDRESS,
      fetchers::tokens::fetch_token_registry
   };

   create_testing!("fetchers::pools");

   run!("fetch_undead_pool_snapshot", {
      let ava = &AVALANCHE;
      let primary = "USDC";
      let reg = now(fetch_token_registry(ava))?;
      let usdc_undead =
         now(fetch_undead_pool_snapshot(ava, TEST_ADDRESS, &reg,
                                        primary, 0.05, 100.0, true))?;
         println!("{primary}+UNDEAD pivot pool:

{}", usdc_undead.status());
   });
}
