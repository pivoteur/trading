use std::pin::Pin;

use chrono::NaiveDate;

use book::{ err_utils::ErrStr, list_utils::async_filter_map };
use libs::types::{
   blockchains::Blockchain,
   comps::{ Composition, mk_composition },
   quotes::{ Quotes, mk_quotes },
   tokens::coins::{ Coin, mk_coin },
   util::Token
};
use super::{ auto_trader::query_quote, types::tokens::TokenRegistry };

pub async fn compute_trade_amounts(chain: &Blockchain, date: &NaiveDate,
                                   registry: &TokenRegistry,
                                   amounts: &[(Token, f32)], debug: bool)
      -> ErrStr<(Composition, Quotes)> {
   let coins_n_prices =
      async_filter_map(compute_coin(chain, date, registry, debug),
                       amounts.to_vec()).await?;
   if let [(coin1, price1), (coin2, price2)] = coins_n_prices.as_slice() {
      let coins = &[coin1.clone(), coin2.clone()];
      let prices = &[price1, price2];
      let aliased_prices: Vec<(&str, f32)> =
         prices.into_iter().map(|(a,b)| (a.as_str(), *b)).collect();
      let quotes = mk_quotes(date, &aliased_prices);
      let hard_top = from_assets(coins, debug)?;
      Ok((hard_top, quotes))
   } else {
      Err(s("Amounts not a pair of token-prices"))
   }
}

fn compute_coin<'a>(chain: &'a Blockchain, date: &'a NaiveDate,
                    registry: &'a TokenRegistry, debug: bool)
      -> impl Fn((String, f32))
      -> Pin<Box<dyn Future<Output = ErrStr<(Coin, (String, f32))>> + 'a>> {
   move | (token, max_amt): (String, f32) | Box::pin(async move {
      let quote = query_quote(chain, registry, &token, debug).await?;
      let coin =
         mk_coin(&(chain.clone(), token.clone()), max_amt, &quote, date);
      Ok((coin, (token, quote.amount())))
   })
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_test {
   use super::*;
   use paste::paste;
   use book::{ create_testing, date_utils::yesterday, utils::now };
   use libs::types::blockchains::Blockchain::AVALANCHE;
   use crate::fetchers::tokens::fetch_token_registry;

   create_testing!("processors");

   run!("compute_trade_amounts", {
      let ava = &AVALANCHE;
      let yday = &yesterday();
      let registry = now(fetch_token_registry(ava))?;
      let amounts = vec!([(s("BTC"), 0.1), (s("ETH"), 3.4)]);
      let (composition, quotes) =
         now(compute_trade_amounts(ava, yday, &registry, &amounts))?;
