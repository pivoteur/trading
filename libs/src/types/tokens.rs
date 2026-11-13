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
