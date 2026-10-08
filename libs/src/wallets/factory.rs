use std::pin::Pin;

use book::err_utils::ErrStr;
use libs::types::blockchains::{
   Blockchain,
   Blockchain::AVALANCHE,
   Blockchain::BINANCE
};

use super::{
   avalanche::mk_connection_to_avalanche,
   binance::mk_connection_to_binance,
   mock::mock_connection
};

use crate::{
   fetchers::tokens::fetch_token_registry,
   types::{
      modes::execution::{ Execution, Execution::DRYRUN },
      wallets::Wallet
   }
};

pub fn mk_connector<'a>(mode: &'a Execution)
      -> impl Fn(&'a Blockchain, &'a str, &'a str, bool)
      -> Pin<Box<dyn Future<Output = ErrStr<Box<dyn Wallet>>> + 'a>> {
   move | chain: &'a Blockchain, wallet_address: &'a str,
          keystore_path: &'a str, debug: bool | 
      Box::pin(async move { if mode == &DRYRUN {
         Ok(mock_connection(chain, debug))
      } else {
         connect_wallet(chain, wallet_address, keystore_path, debug).await
      }})
}

pub async fn connect_wallet(blockchain: &Blockchain, wallet_address: &str,
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
   use crate::types::modes::execution::Execution::LIVE;

   create_testing!("wallets::factory");

   run!("connect_avax_wallet", " (mock wallet)", {
      let wallet =
         now(mk_connector(&DRYRUN)(&AVALANCHE, "0x123", "abc", true))?;
      println!("Connected Avalanche wallet:\n\n{wallet}");
   });

   run!("connect_binance_wallet",
        " (live wallet (no real address nor keystore))", {
      let wallet = now(mk_connector(&LIVE)(&BINANCE, "0xabc", "xyz", false))?;
      println!("Connected Binance wallet:\n\n{wallet}");
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use libs::types::blockchains::Blockchain::ETHEREUM;
   use crate::types::modes::execution::Execution::LIVE;

   #[tokio::test] async fn fail_connect_to_ethereum() {
      let wallet = connect_wallet(&ETHEREUM, "0xa1b2c3", "path", true).await;
      assert!(wallet.is_err(), "Should not connect to Ethereum");
   }

   #[tokio::test] async fn test_connect_mock_wallet() -> ErrStr<()> {
      let wallet = mk_connector(&DRYRUN)(&BINANCE, "0x789", "pqr", true).await?;
      assert!(wallet.wallet_address().contains("mock"), 
              "wallet {wallet} is live!");
      Ok(())
   }

   #[tokio::test] async fn test_connect_live_wallet() -> ErrStr<()> {
      let wallet =
         mk_connector(&LIVE)(&AVALANCHE, "0xfEd", "stu", false).await?;
      assert_eq!("0xfEd", wallet.wallet_address());
      Ok(())
   }
}
