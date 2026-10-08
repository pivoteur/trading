use std::fmt;
use async_trait::async_trait;

use crate::{
   fetchers::tokens::fetch_token_registry,
   types::{ tokens::TokenRegistry, wallets::Wallet }
};

use book::{ err_utils::ErrStr, string_utils::s };
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };

pub struct Ava {
   address: String,
   keystore_path: String,
   tokens: TokenRegistry,
   debug: bool
}

impl fmt::Debug for Ava {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.debug_struct("Ava")
       .field("address", &self.address)
       .field("keystore_path", &"***")
       .field("tokens", &"Avalance tokens")
       .field("debug", &self.debug)
       .finish()
   }
}

#[async_trait(?Send)]
impl Wallet for Ava {
   fn blockchain(&self) -> &Blockchain { &AVALANCHE }
   fn debug(&self) -> bool { self.debug }
   fn keystore_path(&self) -> &str { &self.keystore_path }
   fn token_registry(&self) -> &TokenRegistry { &self.tokens }
   fn wallet_address(&self) -> &str { &self.address }
    fn fmt_wallet(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // You can use the concrete type's format logic here
        write!(f, "{:?}", self)
    }
}

pub async fn connect_to_avalanche(wallet_address: &str, keystore_path: &str,
                            debug: bool)
      -> ErrStr<Ava> {
   let tokens = fetch_token_registry(&AVALANCHE).await?;
   Ok(mk_connection_to_avalanche(tokens, wallet_address, keystore_path, debug))
}

pub fn mk_connection_to_avalanche(registry: TokenRegistry, wallet_address: &str,
                                  keystore_path: &str, debug: bool) -> Ava {
   Ava { address: s(wallet_address),
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

   create_testing!("wallets::avalanche");

   run!("connect_to_avalanche", {
      let wallet = now(connect_to_avalanche("0x123", "xyz", true))?;
      println!("My Avalanche wallet is\n{wallet:?}");
   });

   run!("btc_quote", {
      let wallet = now(connect_to_avalanche("0x123", "xyz", true))?;
      let btc = now(wallet.quote("btc"))?;
      println!("The quote for BTC is {btc}");
   });

   run!("avax_quote", {
      let wallet = now(connect_to_avalanche("0x123", "xyz", true))?;
      let avax = now(wallet.quote("avax"))?;
      println!("The quote for AVAX is {avax}");
   });
}
