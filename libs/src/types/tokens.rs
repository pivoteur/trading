use std::collections::HashMap;

use serde::{ Deserialize, Serialize };

use book::{
   csv_utils::{ CsvWriter, CsvHeader, as_csv },
   err_utils::ErrStr,
   string_utils::s
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TokenEntry {
    #[serde(default)]
    pub native:   bool,
    #[serde(default)]
    pub address:  Option<String>,
    pub decimals: usize
}

#[derive(Debug)]
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
