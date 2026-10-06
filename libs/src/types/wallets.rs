use std::fmt;

use async_trait::async_trait;

use book::{ currency::usd::USD, err_utils::ErrStr };
use libs::types::blockchains::Blockchain;
use super::{ balances::tokens::TokenBalance, tokens::TokenRegistry };
use crate::{
   auto_trading::query_quote,
   fetchers::wallets::fetch_wallet_balances
};

#[async_trait(?Send)]
pub trait Wallet: Sync {
   async fn quote(&self, token: &str) -> ErrStr<USD> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      query_quote(chain, tokens, token, self.debug()).await
   }

   async fn balances(&self) -> ErrStr<Vec<TokenBalance>> {
      let chain = self.blockchain();
      let tokens = self.token_registry();
      let addy = self.wallet_address();
      fetch_wallet_balances(chain, tokens, addy, self.debug()).await
   }

   async fn send(&self) -> ErrStr<()>;
   async fn trade(&self) -> ErrStr<()>;
   fn blockchain(&self) -> &Blockchain;
   fn token_registry(&self) -> &TokenRegistry;
   fn debug(&self) -> bool { false }
   fn keystore_path(&self) -> &str;
   fn wallet_address(&self) -> &str;
   fn fmt_wallet(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
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
