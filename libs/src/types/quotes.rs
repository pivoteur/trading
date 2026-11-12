use chrono::NaiveDateTime;
use libs::types::quotes::Quotes;

use super::{ tokens::TokenRegistry, wallet::Wallet };

use crate::implementation::quotes::query_quotes;

const FLUSH: i64 = 15;

pub struct ActiveQuotes {
   last_updated: NaiveDateTime,
   quotes: Quotes
}

impl ActiveQuotes {
   pub async fn lookup(&mut self, token: &str) -> ErrStr<f32> {
      let now = NaiveDateTime::now();
      let dur = now - self.last_updated;
      let secs = dur.num_seconds();
      if secs > FLUSH {
         not_implemented!("ActiveQuotes::lookup-refresh", token)
      } else {
         quotes.lookup(token)
      }
   }
}

pub fn mk_active_quotes(wallet: &Box<dyn Wallet>) -> ErrStr<ActiveQuotes> {
   let now = NaiveDateTime::now();
   let registry = wallet.token_registry();
