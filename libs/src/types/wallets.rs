use std::fmt;

use async_trait::async_trait;
use chrono::NaiveDate;

use book::{
   debug,
   not_implemented,
   currency::usd::USD,
   err_utils::ErrStr,
   string_utils::s
};
use libs::{ collections::assets::Assets, types::blockchains::Blockchain };
use super::{ balances::tokens::as_assets, tokens::TokenRegistry };
use crate::{
   auto_trading::{ query_quote, send_tokens_to_address },
   fetchers::wallets::fetch_wallet_balances
};

#[async_trait(?Send)]
pub trait Wallet: Sync {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      query_quote(chain, tokens, token, self.debug()).await
   }

   async fn balances(&self, date: &NaiveDate) -> ErrStr<Assets> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      let addy = self.wallet_address();
      let debug = self.debug();
      let bals = fetch_wallet_balances(chain, tokens, addy, debug).await?;
      Ok(as_assets(&bals, chain, date))
   }

   async fn send(&self, token: &str, to_address: &str, amount: f32)
         -> ErrStr<()> {
      let debug = self.debug();
      debug!("send", debug);
      let log_line =
         format!("{} send {amount:.8} {token} -> {to_address}",
                 self.mode());
      log!("mode {}", log_line);
      log!("wallet {}", self.wallet_address());
      self.do_send(token, to_address, amount).await
   }

   async fn do_send(&self, token: &str, to_address: &str, amount: f32)
         -> ErrStr<()> {
      let debug = self.debug();
      debug!("do_send", debug);
      let (this, that) =
         send_tokens_to_address(self.blockchain(), self.wallet_address(),
                                self.token_registry(), token, to_address,
                                amount, self.keystore_path(), debug).await?;
      log!("Result from send: {} {}", this, that);
      Ok(())
   }
   async fn trade(&self) -> ErrStr<()> {
      not_implemented!("trade")
   }
   fn blockchain(&self) -> &Blockchain;
   fn token_registry(&self) -> &TokenRegistry;
   fn debug(&self) -> bool { false }
   fn keystore_path(&self) -> &str;
   fn wallet_address(&self) -> &str;
   fn fmt_wallet(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
   fn mode(&self) -> String { s("LIVE") }
}

// 2. Implement fmt::Display for the dyn trait object itself
impl fmt::Display for dyn Wallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Delegate the formatting to the inner concrete type's method
        self.fmt_wallet(f)
    }
}

// Each implementation is responsible for their own constructor
// which then the factory instantiates, parameterized by blockchain
