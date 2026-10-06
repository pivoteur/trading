use book::{ currency::usd::USD, err_utils::ErrStr };
use libs::types::blockchains::Blockchain;
use super::{ balances::tokens::TokenBalance, tokens::TokenRegistry };
use crate::{
   auto_trading::query_quote,
   fetchers::wallets::fetch_wallet_balances
};

pub trait Wallet {
   fn quote(&self, token: &str) -> impl Future<Output = ErrStr<USD>> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      query_quote(chain, tokens, token, self.debug())
   }

   fn balances(&self) -> impl Future<Output = ErrStr<Vec<TokenBalance>>> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      let addy = self.wallet_address();
      fetch_wallet_balances(chain, tokens, addy, self.debug())
   }

   fn send(&self) -> impl Future<Output = ErrStr<()>>;
   fn trade(&self) -> impl Future<Output = ErrStr<()>>;
   fn blockchain(&self) -> &Blockchain;
   fn token_registry(&self) -> &TokenRegistry;
   fn debug(&self) -> bool { false }
   fn keystore_path(&self) -> &str;
   fn wallet_address(&self) -> &str;
}

// Each implementation is responsible for their own constructor
