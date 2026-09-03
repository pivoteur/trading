use std::collections::HashMap;

use serde::Deserialize;

use book::{ err_utils::ErrStr, rest_utils::read_rest };
use libs::types::blockchains::Blockchain;
use super::path_utils::token_url;

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================

#[derive(Debug, Deserialize, Clone, PartialEq)]
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

pub async fn load_tokens(blockchain: Blockchain) -> ErrStr<TokenRegistry> {
   let url = token_url(blockchain);
   let raw = read_rest(&url).await?;
   parse_token_registry(&raw)
}

/// Each blockchain has its own tokens in a toml (the token
/// set differs per blockchain) and passes the raw string here to parse it.
pub fn parse_token_registry(toml_str: &str) -> ErrStr<TokenRegistry> {
    let tokens = toml::from_str(toml_str)
         .map_err(|e| format!("Failed to parse tokens.toml: {e}"))?;
    Ok(TokenRegistry { tokens })
}

impl TokenRegistry {
   pub fn token(&self, symbol: &str) -> ErrStr<TokenEntry> {
      self.tokens.get(symbol).ok_or(format!("No entry for {symbol}")).cloned()
   }
}

