use std::collections::HashMap;

use serde::{ Deserialize, Serialize };

use book::{
   csv_utils::{ CsvWriter, CsvHeader, as_csv },
   err_utils::ErrStr,
   rest_utils::read_rest,
   string_utils::s
};
use libs::types::blockchains::Blockchain;
use super::path_utils::token_url;

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TokenEntry {
    #[serde(default)]
    pub native:   bool,
    #[serde(default)]
    pub address:  Option<String>,
    pub decimals: u32
}

#[derive(Debug)]
pub struct TokenRegistry {
   tokens: HashMap<String, TokenEntry>
}

pub async fn load_tokens(blockchain: &Blockchain) -> ErrStr<TokenRegistry> {
   let url = token_url(blockchain);
   let raw = read_rest(&url).await?;
   parse_token_registry(&raw)
}

/// Each blockchain has its own tokens in a toml (the token
/// set differs per blockchain) and passes the raw string here to parse it.
fn parse_token_registry(toml_str: &str) -> ErrStr<TokenRegistry> {
    let tokens = toml::from_str(toml_str)
         .map_err(|e| format!("Failed to parse tokens.toml: {e}"))?;
    Ok(TokenRegistry { tokens })
}

impl TokenRegistry {
   pub fn token(&self, symbol: &str) -> ErrStr<TokenEntry> {
      self.tokens.get(&symbol.to_uppercase())
                 .ok_or(format!("No entry for {symbol}")).cloned()
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
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, csv_utils::print_csv, utils::now };
   use libs::types::blockchains::Blockchain::*;

   create_testing!("libs::tokens");

   run!("load_tokens_avalanche", {
      let toks = now(load_tokens(&AVALANCHE))?;
      print_csv(&toks);
   });

   run!("load_tokens_binance", {
      let toks = now(load_tokens(&BINANCE))?;
      print_csv(&toks);
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use libs::types::blockchains::Blockchain::*;

   #[tokio::test] async fn test_load_tokens_avax_native() -> ErrStr<()> {
      let toks = load_tokens(&AVALANCHE).await?;
      assert!(toks.token("avax")?.native);
      Ok(())
   }

   #[tokio::test] async fn test_load_tokens_binance_native() -> ErrStr<()> {
      let toks = load_tokens(&BINANCE).await?;
      assert!(toks.token("bnb")?.native);
      Ok(())
   }

   #[tokio::test] async fn test_btc_has_addy() -> ErrStr<()> {
      let toks = load_tokens(&AVALANCHE).await?;
      let btc_mb_addy = toks.token("btc")?.address;
      assert!(btc_mb_addy.is_some());
      btc_mb_addy.and_then(|btc_addy| {
         assert!(btc_addy.starts_with("0x"));
         Some(())
      });
      Ok(())
   }
}
