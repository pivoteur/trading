use std::collections::HashMap;

use chrono::NaiveDate;

use serde::{ Deserialize, Serialize };

use book::{
   csv_utils::{ CsvWriter, CsvHeader, as_csv },
   err_utils::ErrStr,
   string_utils::s,
   types::values::Value
};

use swerve::{ fetch_prices::fetch_prices, types::{ QuoteDict, mk_token } };

use libs::types::quotes::{ Quotes, mk_quotes };

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TokenEntry {
    #[serde(default)]
    pub native:   bool,
    #[serde(default)]
    pub address:  Option<String>,
    pub decimals: usize,
    pub api_id: String
}

#[derive(Clone, Debug)]
pub struct TokenRegistry {
   tokens: HashMap<String, TokenEntry>
}

pub fn mk_token_registry(tokens: HashMap<String, TokenEntry>) -> TokenRegistry {
   TokenRegistry { tokens }
}

impl TokenRegistry {
   pub fn token(&self, symbol: &str) -> ErrStr<TokenEntry> {
      self.tokens.get(&symbol.to_uppercase())
                 .ok_or(format!("No entry for {symbol}")).cloned()
   }
   pub fn as_map(&self) -> HashMap<String, TokenEntry> { self.tokens.clone() }
   fn as_quote_dict(&self) -> QuoteDict {
      self.tokens.iter()
          .map(|(tok, entry)| (entry.api_id.to_string(), mk_token(tok)))
          .collect()
   }
   pub async fn fetch_quotes(&self, auth: &str, date: &NaiveDate)
         -> ErrStr<Quotes> {
      let dict = self.as_quote_dict();
      let prices = fetch_prices(auth, &dict).await?;
      let quotes: Vec<(&str, f32)> =
         prices.iter().map(|(tok, qt)| (tok.as_ref(), qt.value())).collect();
      Ok(mk_quotes(date, &quotes))
   }
}

impl CsvWriter for TokenRegistry {
   fn ncols(&self) -> usize { 4 }
   fn as_csv(&self) -> String {
      let vals: Vec<TokenEntry> = self.tokens.values().cloned().collect();
      let vals_csv =
         as_csv(&vals, false).expect("Error parsing tokens for write");
      let daterz: Vec<String> = vals_csv.split("\n").map(s).collect();
      let rows: &Vec<String> =
         &self.tokens.keys()
                     .zip(daterz.iter())
                     .map(|(k,v)| format!("{k},{v}"))
                     .collect();
      rows.join("\n")
   }
}

impl CsvHeader for TokenRegistry {
   fn header(&self) -> String { s("token,native?,address,decimals") }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod sample_data {
   use super::*;
   use book::{ csv_utils::list_csv, date_utils::yesterday, utils::get_env };

   fn mk_entry<'a>(tok: &'a str, native: bool, api_id: &'a str)
         -> (&'a str, TokenEntry) {
      (tok, TokenEntry {
          native,
          address: None,
          decimals: 18,
          api_id: s(api_id)
       })
   }
   fn mk_registry(toks: &[(&str, TokenEntry)]) -> TokenRegistry {
      let tokens: HashMap<String, TokenEntry> =
         toks.iter().map(|(tok, entry)| (s(tok), entry.clone())).collect();
      mk_token_registry(tokens)
   }
   pub fn binance_registry() -> TokenRegistry {
      mk_registry(&[mk_entry("BNB", true, "binancecoin"),
                    mk_entry("LTC", false, "litecoin"),
                    mk_entry("DOGE", false, "dogecoin")])
   }
   pub fn avalanche_registry() -> TokenRegistry {
      mk_registry(&[mk_entry("AVAX", true, "avalanche-2"),
                    mk_entry("BTC", false, "bitcoin"),
                    mk_entry("ETH", false, "ethereum"),
                    mk_entry("UNDEAD", false, "undead-blocks")])
   }
   pub async fn quotes_report(blockchain: &str, reg: &TokenRegistry)
         -> ErrStr<String> {
      let auth = get_env("COIN_GECKO_API_KEY")?;
      let quotes = reg.fetch_quotes(&auth, &yesterday()).await?;
      Ok(format!("{blockchain} quotes:\n\n{}", list_csv(&[quotes], true)))
   }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use super::sample_data::*;
   use paste::paste;
   use book::{ create_testing, utils::now };

   create_testing!("types::tokens");

   async fn quotes(blockchain: &str, reg: &TokenRegistry) -> ErrStr<()> {
      println!("{}", quotes_report(blockchain, reg).await?);
      Ok(())
   }
   run!("avalanche_quotes", now(quotes("Avalanche", &avalanche_registry()))?);
   run!("binance_quotes", now(quotes("Binance", &binance_registry()))?);
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use super::sample_data::*;

   #[tokio::test] async fn test_binance_registry_has_bnb_quote() -> ErrStr<()> {
      let binance_quotes = quotes_report("Binance", &binance_registry()).await?;
      assert!(binance_quotes.contains("BNB"));
      Ok(())
   }

   #[tokio::test] async fn test_avalanche_registry_has_avax_quote()
         -> ErrStr<()> {
      let avalanche_quotes =
         quotes_report("Avalanche", &avalanche_registry()).await?;
      assert!(avalanche_quotes.contains("AVAX"));
      Ok(())
   }
}
