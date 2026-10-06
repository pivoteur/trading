use book::err_utils::ErrStr;
use libs::types::blockchains::{
   Blockchain,
   Blockchain::AVALANCHE,
   Blockchain::BINANCE
};

use super::{
   avalanche::mk_connection_to_avalanche,
   binance::mk_connection_to_binance
};

use crate::{
   fetchers::tokens::fetch_token_registry,
   types::wallets::Wallet
};

pub async fn connect_wallet(blockchain: &Blockchain,
                            wallet_address: &str,
                            keystore_path: &str, debug: bool)
      -> ErrStr<Box<dyn Wallet>> {
   let registry = fetch_token_registry(blockchain).await?;
   match blockchain {
      &AVALANCHE =>
         Ok(Box::new(mk_connection_to_avalanche(registry, wallet_address,
                                       keystore_path, debug))),
      &BINANCE =>
         Ok(Box::new(mk_connection_to_binance(registry, wallet_address,
                                     keystore_path, debug))),
      _ => Err(format!("Blockchain {blockchain} not supported"))
   }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };

   create_testing!("wallets::factory");

   run!("connect_avax_wallet", {
      let wallet = now(connect_wallet(&AVALANCHE, "0x123", "abc", true))?;
      println!("Connected Avalanche wallet:\n\n{wallet}");
   });

   run!("connect_binance_wallet", {
      let wallet = now(connect_wallet(&BINANCE, "0xabc", "xyz", false))?;
      println!("Connected Binance wallet:\n\n{wallet}");
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use libs::types::blockchains::Blockchain::ETHEREUM;

   #[tokio::test] async fn fail_connect_to_ethereum() {
      let wallet = connect_wallet(&ETHEREUM, "0xa1b2c3", "path", true).await;
      assert!(wallet.is_err(), "Should not connect to Ethereum");
   }
}
