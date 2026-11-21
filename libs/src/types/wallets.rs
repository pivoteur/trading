use std::fmt;

use async_trait::async_trait;
use chrono::NaiveDate;

use book::{
   debug,
   currency::usd::{ USD, mk_usd },
   err_utils::ErrStr,
   string_utils::s,
   utils::get_env
};
use libs::{
   collections::assets::Assets,
   types::{ blockchains::Blockchain, quotes::Quotes }
};
use super::{
   balances::tokens::as_assets,
   quotes::fetch_active_quotes,
   tokens::TokenRegistry,
   trades::Trade
};
use crate::{
   consts::COIN_GECKO_API_KEY,
   implementation::{
      fetchers::wallets::fetch_wallet_balances,
      transactions::{
         senders::send_tokens_to_address,
         traders::{ check_floor, trade as execute_trade }
      }
   }
};

#[async_trait(?Send)]
pub trait Wallet: Sync {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      let auth = get_env(COIN_GECKO_API_KEY)?;
      let quotes = quotes(self, &auth).await?;
      let qt = quotes.lookup(token)?;
      Ok(mk_usd(qt))
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
      let min = min_floor.unwrap_or_default();
      let query =
         check_floor(self.blockchain(), self.token_registry(), from, to, amount,
                     min, slippage, self.debug()).await?;
      self.do_trade(from, amount, to, min, slippage, query).await
   }

   async fn do_trade(&self, from: &str, amount: f32, to: &str,
                     min_floor: f32, slippage: u16, swap: f32)
         -> ErrStr<Trade> {
      execute_trade(self.blockchain(), self.wallet_address(),
                    self.token_registry(), from, amount, to, min_floor,
                    slippage, swap, self.keystore_path(), self.debug()).await
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

pub async fn quotes<W: Wallet + ?Sized>(wallet: &W, auth: &str)
      -> ErrStr<Quotes> {
   let chain = wallet.blockchain();
   let tokens = wallet.token_registry();
   let debug = wallet.debug();
   fetch_active_quotes(tokens, chain, auth, debug).await
}
