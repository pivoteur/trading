use std::fmt;

use crate::{
   fetchers::{ tokens::fetch_token_registry },
   types::{ tokens::TokenRegistry, wallets::Wallet }
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
   async fn quote(&self) -> ErrStr<USD> {
      not_implemented!("quote")
   }
   async fn balances(&self) -> ErrStr<()> {
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

   use crate::types::wallets::Wallet;

   pub struct MockAva;

impl fmt::Debug for MockAva {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.debug_struct("MockAva")
       .field("address", &"mocked")
       .field("keystore_path", &"***")
       .field("tokens", &"Avalanche tokens")
       .finish()
   }
}

impl Wallet for MockAva {
   async fn quote(&self) -> ErrStr<USD> {
      Ok(mk_usd(1.23))
   }
   async fn balances(&self) -> ErrStr<()> {
      not_implemented!("balances")
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
      Ok(MockAva { })
   }
}
      
// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::mocks::*;
   use paste::paste;
   use book::{ create_testing, err_utils::ErrStr, utils::now };

   create_testing!("wallets::avalanche");

   run!("connect_to_avalanche", {
      let wallet = now(connect_to_avalanche("0x123", "xyz"))?;
      println!("My Avalanche wallet is:\n{wallet:?}");
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::mocks::*;
   use book::err_utils::ErrStr;
   use crate::types::wallets::Wallet;

   #[tokio::test] async fn test_quote_dollaz() -> ErrStr<()> {
      let wallet = connect_to_avalanche("0x123", "xyz").await?;
      let avax = wallet.quote().await?;
      assert_eq!(avax.amount(), 1.23);
      Ok(())
   }
}
