use std::fmt;

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::{
   auto_trading::query_quote,
   fetchers::tokens::fetch_token_registry,
   types::{
      balances::tokens::{ mk_token_balance, as_assets },
      modes::execution::{ Execution, Execution::DRYRUN },
      tokens::TokenRegistry,
      wallets::Wallet
   }
};

use book::{
   not_implemented,
   currency::usd::{ USD, mk_usd },
   err_utils::ErrStr
};

use libs::{
   collections::assets::Assets,
   types::blockchains::{ Blockchain, Blockchain::AVALANCHE }
};

// ----- MOCKS -------------------------------------------------------

#[derive(Debug)]
pub struct MockWallet { blockchain: Blockchain, debug: bool }

#[async_trait(?Send)]
impl Wallet for MockWallet {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      let chain = &self.blockchain;
      let tokens = fetch_token_registry(chain).await?;
      query_quote(chain, &tokens, token, self.debug).await
   }
   async fn balances(&self, date: &NaiveDate) -> ErrStr<Assets> {
      let (token, price) = (self.blockchain.protocol_token(),
            if self.blockchain == AVALANCHE { 11.11 } else { 767.32 });
      let balances = [mk_token_balance("BTC", mk_usd(83456.0), 1.0),
                      mk_token_balance("ETH", mk_usd(2734.1), 34.0),
                      mk_token_balance("UNDEAD", mk_usd(0.00054), 500000.0),
                      mk_token_balance(&token, mk_usd(price), 0.5)];
      Ok(as_assets(&balances, self.blockchain(), date))
   }
   async fn do_send(&self, _token: &str, _to: &str, _amt: f32) -> ErrStr<()> {
      // a do-nothing method
      Ok(())
   }
   fn blockchain(&self) -> &Blockchain { &self.blockchain }
   fn keystore_path(&self) -> &str { not_implemented!("keystore_path") }
   fn wallet_address(&self) -> &str { "0xmock_address" }
   fn debug(&self) -> bool { true }
   fn token_registry(&self) -> &TokenRegistry {
      not_implemented!("token_registry")
   }
   fn mode(&self) -> Execution { DRYRUN }
   fn fmt_wallet(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      // You can use the concrete type's format logic here
      write!(f, "{:?}", self)
   }
}

pub fn mock_connection(blockchain: &Blockchain, debug: bool)
      -> Box<dyn Wallet> {
   Box::new(MockWallet { blockchain: blockchain.clone(), debug })
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{
      create_testing,
      csv_utils::list_csv,
      date_utils::yesterday,
      utils::now
   };
   use libs::types::blockchains::Blockchain::BINANCE;

   create_testing!("wallets::mock");

   run!("connection_avalanche", " (mock)", {
      let wallet = mock_connection(&AVALANCHE, true);
      println!("My Avalanche wallet is:\n{wallet}");
   });
   run!("connection_binance", " (mock)", {
      let wallet = mock_connection(&BINANCE, true);
      println!("My Binance wallet is:\n{wallet}");
   });
   run!("balances_avalanche", " (mock)", {
      let wallet = mock_connection(&AVALANCHE, true);
      let yday = &yesterday();
      let balances = now(wallet.balances(yday))?;
      println!("Avalanche Wallet balances:\n{}", list_csv(&[balances], true));
   });
   run!("balances_binance", " (mock)", {
      let wallet = mock_connection(&BINANCE, true);
      let yday = &yesterday();
      let balances = now(wallet.balances(yday))?;
      println!("Binance Wallet balances:\n{}", list_csv(&[balances], true));
   });
   run!("quotes_avalanche", " (mock)", {
      let wallet = mock_connection(&AVALANCHE, true);
      let quote = now(wallet.quote("AVAX"))?;
      println!("AVAX quote: {quote}");
   });
   run!("quotes_binance", " (mock)", {
      let wallet = mock_connection(&BINANCE, true);
      let quote = now(wallet.quote("BNB"))?;
      println!("BNB quote: {quote}");
   });
}
