use std::fmt;

use crate::{
   auto_trading::query_quote,
   fetchers::{ tokens::fetch_token_registry },
   types::{
      balances::tokens::TokenBalance,
      tokens::TokenRegistry,
      wallets::Wallet
   }
};

use book::{
   not_implemented,
   currency::usd::USD,
   err_utils::ErrStr,
   string_utils::s
};

use libs::types::blockchains::Blockchain::AVALANCHE;

pub struct Ava {
   address: String,
   keystore_path: String,
   tokens: TokenRegistry
}

impl fmt::Debug for Ava {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.debug_struct("Ava")
       .field("address", &self.address)
       .field("keystore_path", &"***")
       .field("tokens", &"Avalance tokens")
       .finish()
   }
}

impl Wallet for Ava {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      query_quote(&AVALANCHE, &self.tokens, token, false).await
   }
   async fn balances(&self) -> ErrStr<Vec<TokenBalance>> {
      not_implemented!("balances")
   }
   async fn send(&self) -> ErrStr<()> {
      not_implemented!("send")
   }
   async fn trade(&self) -> ErrStr<()> {
      not_implemented!("trade")
   }
}

pub async fn connect_to_avalanche(addy: &str, keystore_path: &str)
      -> ErrStr<Ava> {
   let tokens = fetch_token_registry(&AVALANCHE).await?;
   Ok(Ava { address: s(addy), keystore_path: s(keystore_path), tokens })
}

// ----- MOCKS -------------------------------------------------------

pub mod mocks {
   use std::fmt;
   use book::{
      not_implemented,
      currency::usd::{ USD, mk_usd },
      err_utils::ErrStr
   };

   use crate::types::{
      balances::tokens::{ TokenBalance, mk_token_balance },
      wallets::Wallet
   };

   #[derive(Default)]
   pub struct MockAva;

   impl fmt::Debug for MockAva {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
         f.debug_struct("MockAva")
          .field("address", &"mocked")
          .field("keystore_path", &"***")
          .field("tokens", &"Avalanche tokens")
          .field("debug", &true)
          .finish()
      }
   }

   impl Wallet for MockAva {
      async fn quote(&self, _token: &str) -> ErrStr<USD> {
         Ok(mk_usd(1.23))
      }
      async fn balances(&self) -> ErrStr<Vec<TokenBalance>> {
         Ok(vec![mk_token_balance("BTC", mk_usd(83456.0), 1.0),
                 mk_token_balance("ETH", mk_usd(2734.1), 34.0),
                 mk_token_balance("AVAX", mk_usd(11.11), 0.5)])
      }
      async fn send(&self) -> ErrStr<()> {
         not_implemented!("send")
      }
      async fn trade(&self) -> ErrStr<()> {
         not_implemented!("trade")
      }
   }

   pub async fn connect_to_avalanche(_addy: &str, _keystore_path: &str)
         -> ErrStr<MockAva> {
      Ok(MockAva::default())
   }

   // ----- TESTS -------------------------------------------------------

   #[cfg(test)]
   #[cfg(not(tarpaulin_include))]
   mod functional_tests {
      use super::*;
      use paste::paste;
      use book::{
         create_testing,
         csv_utils::as_csv,
         err_utils::ErrStr,
         utils::now
      };

      create_testing!("wallets::mocks::avalanche");

      run!("connect_to_avalanche", " (mock)", {
         let wallet = now(connect_to_avalanche("0x123", "xyz"))?;
         println!("My Avalanche wallet is:\n{wallet:?}");
      });
      run!("balances", " (mock)", {
         let wallet = now(connect_to_avalanche("0x123", "xyz"))?;
         let balances = now(wallet.balances())?;
         println!("Wallet balances:\n{}", as_csv(&balances, true)?);
      });
   }

   #[cfg(test)]
   #[cfg(not(tarpaulin_include))]
   mod tests {
      use super::*;
      use book::err_utils::ErrStr;
      use crate::types::wallets::Wallet;

      #[tokio::test] async fn test_quote_avax() -> ErrStr<()> {
         let wallet = connect_to_avalanche("0x123", "xyz").await?;
         let avax = wallet.quote("AVAX").await?;
         assert_eq!(avax.amount(), 1.23);
         Ok(())
      }
   }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };

   create_testing!("wallets::avalanche");

   run!("connect_to_avalanche", {
      let wallet = now(connect_to_avalanche("0x123", "xyz"))?;
      println!("My Avalanche wallet is\n{wallet:?}");
   });

   run!("btc_quote", {
      let wallet = now(connect_to_avalanche("0x123", "xyz"))?;
      let btc = now(wallet.quote("btc"))?;
      println!("The quote for BTC is {btc}");
   });
}
