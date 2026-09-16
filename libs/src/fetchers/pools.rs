use book::err_utils::ErrStr;
use libs::types::blockchains::Blockchain;
use super::wallets::fetch_wallet_balance;
use crate::{
   consts::UNDEAD,
   types::{ balances::BalanceSnapshot, tokens::TokenRegistry }
};

pub async fn fetch_pool_balance(blockchain: &Blockchain, addy: &str,
                                registry: &TokenRegistry, prim: &str,
                                committed: f64, undead_committed: f64)
      -> ErrStr<BalanceSnapshot> {
    // Two independent reads
    let asset_balance =
       fetch_wallet_balance(blockchain, addy, prim, registry).await?;
    let undead_balance =
       fetch_wallet_balance(blockchain, addy, UNDEAD, registry).await?;
    Ok(BalanceSnapshot {
        asset_balance,
        asset_committed: committed,
        asset_available: asset_balance - committed,
        undead_balance,
        undead_committed,
        undead_available: undead_balance - undead_committed,
    })
}

