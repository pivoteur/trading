use book::err_utils::ErrStr;
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };

use super::{
   avalanche::mk_connection_to_avalanche,
   // binance::connect_to_binance,
};

use crate::{
   fetchers::tokens::fetch_token_registry,
   types::wallets::Wallet
};

/*
/// Factory Method returning Wallet implementation based by blockchain or Err
pub async fn connect_wallet(mb_blockchain: Option<&Blockchain>,
                            wallet_address: &str,
                            keystore_path: &str, debug: bool)
      -> ErrStr<dyn Wallet> {
   mb_blockchain.ok_or_elsasync_and_then
   Ok(mb_blockchain.then(|blockchain|
        match blockchain {
           &AVALANCHE =>
              Ok(connect_to_avalanche(wallet_address, keystore_path, debug)),
           // &BINANCE => connect_to_binance(wallet_address, debug)?
           _          => Err(format!("Blockchain {blockchain} not supported"))
        }).or(Ok(mock_connection(debug))))
}
*/

pub async fn connect_wallet(blockchain: &Blockchain,
                            wallet_address: &str,
                            keystore_path: &str, debug: bool)
      -> ErrStr<impl Wallet> {
   let registry = fetch_token_registry(blockchain).await?;
   match blockchain {
      &AVALANCHE =>
         Ok(mk_connection_to_avalanche(registry, wallet_address,
                                       keystore_path, debug)),
      _ => Err(format!("Blockchain {blockchain} not supported"))
   }
}
