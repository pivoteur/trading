use std::fmt;

use async_trait::async_trait;
use chrono::NaiveDate;

use book::{
   debug,
   currency::usd::USD,
   err_utils::ErrStr,
   string_utils::s
};
use libs::{ collections::assets::Assets, types::blockchains::Blockchain };
use super::{
   balances::tokens::as_assets,
   tokens::TokenRegistry,
   trades::Trade
};
use crate::implementation::{
   fetchers::wallets::fetch_wallet_balances,
   transactions::{
      quotes::query_quote,
      senders::send_tokens_to_address,
      traders::attempt_trade_with_actual_amount
   }
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
   async fn trade(&self, from: &str, amount: f32, to: &str,
                  min_floor: Option<f32>, slippage: u16) -> ErrStr<Trade> {
      debug!("trade", self.debug());
      let swap = format!("{amount} {from} -> {to}{}",
                         min_floor.and_then(|x| Some(format!(" (min: {x})")))
                                  .unwrap_or_default());
      log!("trading {}", swap);
      self.do_trade(from, amount, to, min_floor, slippage).await
   }

   async fn do_trade(&self, from: &str, amount: f32, to: &str,
                     mb_min_floor: Option<f32>, slippage: u16)
         -> ErrStr<Trade> {
      let min_floor = mb_min_floor.unwrap_or_default();
      let trade = attempt_trade_with_actual_amount(self.blockchain(),
                           self.wallet_address(), self.token_registry(),
                           from, to, amount, min_floor, slippage,
                           self.keystore_path(), self.debug()).await?;
      Ok(trade)
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
