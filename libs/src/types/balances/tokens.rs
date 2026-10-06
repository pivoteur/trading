use chrono::NaiveDate;
use serde::Serialize;
use serde_with::{ serde_as, DisplayFromStr };

use book::{ currency::usd::{ USD, mk_usd }, string_utils::s };
use libs::types::{ blockchains::Blockchain, tokens::coins::{ Coin, mk_coin } };

// ----- TokenBalance -------------------------------------------------------

#[serde_as]
#[derive(Debug, Clone, Serialize)]
pub struct TokenBalance {
   token: String,
   #[serde_as(as = "DisplayFromStr")]
   quote: USD,
   amount: f32,
   #[serde_as(as = "DisplayFromStr")]
   nav: USD
}

pub fn mk_token_balance(tok: &str, quote: USD, amount: f32) -> TokenBalance {
   let nav = mk_usd(quote.amount() * amount);
   TokenBalance { token: s(tok), quote, amount, nav }
}   

impl TokenBalance {
   pub fn as_coin(&self, blockchain: &Blockchain, date: &NaiveDate) -> Coin {
      mk_coin(&(blockchain.clone(), self.token.clone()),
              self.amount, &self.quote, date)
   }
}
