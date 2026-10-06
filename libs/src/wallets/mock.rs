use crate::{
   auto_trading::query_quote,
   fetchers::tokens::fetch_token_registry,
   types::{
      balances::tokens::{ TokenBalance, mk_token_balance },
      tokens::TokenRegistry,
      wallets::Wallet
   }
};

use book::{
   not_implemented,
   currency::usd::{ USD, mk_usd },
   err_utils::ErrStr
};

use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };

// ----- MOCKS -------------------------------------------------------

#[derive(Debug)]
pub struct MockWallet { blockchain: Blockchain, debug: bool }

impl Wallet for MockWallet {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      let chain = &self.blockchain;
      let tokens = fetch_token_registry(chain).await?;
      query_quote(chain, &tokens, token, self.debug).await
   }
   async fn balances(&self) -> ErrStr<Vec<TokenBalance>> {
      let (token, price) = (self.blockchain.protocol_token(),
            if self.blockchain == AVALANCHE { 11.11 } else { 767.32 });
      Ok(vec![mk_token_balance("BTC", mk_usd(83456.0), 1.0),
              mk_token_balance("ETH", mk_usd(2734.1), 34.0),
              mk_token_balance(&token, mk_usd(price), 0.5)])
   }
   async fn send(&self) -> ErrStr<()> {
      not_implemented!("send")
   }
   async fn trade(&self) -> ErrStr<()> {
      not_implemented!("trade")
   }
   fn blockchain(&self) -> &Blockchain { &self.blockchain }
   fn keystore_path(&self) -> &str { not_implemented!("keystore_path") }
   fn wallet_address(&self) -> &str { not_implemented!("wallet_address") }
   fn debug(&self) -> bool { true }
   fn token_registry(&self) -> &TokenRegistry {
      not_implemented!("token_registry")
   }
}

pub fn mock_connection(blockchain: &Blockchain, debug: bool) -> MockWallet {
   MockWallet { blockchain: blockchain.clone(), debug }
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
      utils::now
   };
   use libs::types::blockchains::Blockchain::BINANCE;

   create_testing!("wallets::mock");

   run!("connection_avalanche", " (mock)", {
      let wallet = mock_connection(&AVALANCHE, true);
      println!("My Avalanche wallet is:\n{wallet:?}");
   });
   run!("connection_binance", " (mock)", {
      let wallet = mock_connection(&BINANCE, true);
      println!("My Binance wallet is:\n{wallet:?}");
   });
   run!("balances_avalanche", " (mock)", {
      let wallet = mock_connection(&AVALANCHE, true);
      let balances = now(wallet.balances())?;
      println!("Avalanche Wallet balances:\n{}", as_csv(&balances, true)?);
   });
   run!("balances_binance", " (mock)", {
      let wallet = mock_connection(&BINANCE, true);
      let balances = now(wallet.balances())?;
      println!("Binance Wallet balances:\n{}", as_csv(&balances, true)?);
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
