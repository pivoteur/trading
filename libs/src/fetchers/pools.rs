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

pub async fn fetch_pool_balance(blockchain: &Blockchain, addy: &str,
                                registry: &TokenRegistry, prim: &str,
                                committed: f32, undead_committed: f32,
                                debug: bool) -> ErrStr<BalanceSnapshot> {
    let asset_balance =
       fetch_token_balance(blockchain, addy, prim, registry, debug).await?;
    let undead_balance =
       fetch_token_balance(blockchain, addy, UNDEAD, registry, debug).await?;
    Ok(mk_balance_snapshot(asset_balance, committed,
                           undead_balance, undead_committed))
}

