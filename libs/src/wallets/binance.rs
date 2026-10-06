use std::fmt;
use async_trait::async_trait;

use crate::{
   fetchers::tokens::fetch_token_registry,
   types::{ tokens::TokenRegistry, wallets::Wallet }
};

use book::{ not_implemented, err_utils::ErrStr, string_utils::s };
use libs::types::blockchains::{ Blockchain, Blockchain::BINANCE };

pub struct Bsc {
   address: String,
   keystore_path: String,
   tokens: TokenRegistry,
   debug: bool
}

impl fmt::Debug for Bsc {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.debug_struct("Bsc")
       .field("address", &self.address)
       .field("keystore_path", &"***")
       .field("tokens", &"Binance tokens")
       .field("debug", &self.debug)
       .finish()
   }
}

#[async_trait(?Send)]
impl Wallet for Bsc {
   async fn send(&self) -> ErrStr<()> {
      let keystore = &self.keystore_path;
      not_implemented!("send", keystore)
   }
   async fn trade(&self) -> ErrStr<()> {
      not_implemented!("trade")
   }
   fn blockchain(&self) -> &Blockchain { &BINANCE }
   fn debug(&self) -> bool { self.debug }
   fn keystore_path(&self) -> &str { &self.keystore_path }
   fn token_registry(&self) -> &TokenRegistry { &self.tokens }
   fn wallet_address(&self) -> &str { &self.address }
    fn fmt_wallet(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // You can use the concrete type's format logic here
        write!(f, "{:?}", self)
    }
}

pub async fn connect_to_binance(wallet_address: &str, keystore_path: &str,
                                debug: bool) -> ErrStr<Bsc> {
   let tokens = fetch_token_registry(&BINANCE).await?;
   Ok(mk_connection_to_binance(tokens, wallet_address, keystore_path, debug))
}

pub fn mk_connection_to_binance(registry: TokenRegistry, wallet_address: &str,
                                keystore_path: &str, debug: bool) -> Bsc {
   Bsc { address: s(wallet_address),
         keystore_path: s(keystore_path),
         tokens: registry,
         debug }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };

   create_testing!("wallets::binance");

   run!("connect_to_binance", {
      let wallet = now(connect_to_binance("0x123", "xyz", true))?;
      println!("My Binance wallet is\n{wallet:?}");
   });

   run!("eth_quote", {
      let wallet = now(connect_to_binance("0x123", "xyz", true))?;
      let eth = now(wallet.quote("eth"))?;
      println!("The quote for ETH is {eth}");
   });

   run!("bnb_quote", {
      let wallet = now(connect_to_binance("0x123", "xyz", true))?;
      let bnb = now(wallet.quote("bnb"))?;
      println!("The quote for BNB is {bnb}");
   });
}
