use serde::Serialize;
use serde_with::{ serde_as, DisplayFromStr };

use book::{ currency::usd::{ USD, mk_usd }, string_utils::s };

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
    

