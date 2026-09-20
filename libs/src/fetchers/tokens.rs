use book::{ err_utils::{ ErrStr, err_or }, rest_utils::read_rest };
use libs::types::blockchains::Blockchain;
use crate::{
   path_utils::token_url,
   types::tokens::{ TokenRegistry, mk_token_registry }
};

//============================================================================
//----- Token Registry --------------------------------------------------------
//============================================================================

pub async fn fetch_token_registry(blockchain: &Blockchain)
      -> ErrStr<TokenRegistry> {
   let url = token_url(blockchain);
   let raw = read_rest(&url).await?;
   parse_token_registry(&raw)
}

/// Each blockchain has its own tokens in a toml (the token
/// set differs per blockchain) and passes the raw string here to parse it.
fn parse_token_registry(toml_str: &str) -> ErrStr<TokenRegistry> {
    let tokens =
       err_or(toml::from_str(toml_str), "Failed to parse tokens.toml")?;
    Ok(mk_token_registry(tokens))
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, csv_utils::print_csv, utils::now };
   use libs::types::blockchains::Blockchain::*;
   use crate::auto_trading::query_swap;

   create_testing!("libs::tokens");

   run!("load_tokens_avalanche", {
      let toks = now(fetch_token_registry(&AVALANCHE))?;
      print_csv(&toks);
   });

   run!("load_tokens_binance", {
      let toks = now(fetch_token_registry(&BINANCE))?;
      print_csv(&toks);
   });

   async fn run_query(prim: &str, piv: &str, amt: f64)
         -> ErrStr<(f64, String)> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let swap =
         query_swap(&AVALANCHE, &registry, prim, piv, amt, true).await?;
      Ok((swap.amount_out, swap.router_address))
   }
      
   run!("live_quote_undead_to_btc", {
      let (amt, addy) = now(run_query("undead", "btc", 5e5))?;
      println!("\t500000 UNDEAD -> {amt:.8} BTC right now (router: {addy})");
   });

    run!("live_quote_btc_to_undead", {
      let (amt, addy) = now(run_query("btc", "undead", 0.005))?;
      println!("\t0.005 BTC -> {amt:.4} UNDEAD right now (router: {addy})");
    });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use libs::types::blockchains::Blockchain::*;

   #[tokio::test] async fn test_fetch_token_registry_avax_native()
         -> ErrStr<()> {
      let toks = fetch_token_registry(&AVALANCHE).await?;
      assert!(toks.token("avax")?.native);
      Ok(())
   }

   #[tokio::test] async fn test_fetch_token_registry_binance_native()
         -> ErrStr<()> {
      let toks = fetch_token_registry(&BINANCE).await?;
      assert!(toks.token("bnb")?.native);
      Ok(())
   }

   #[tokio::test] async fn test_btc_has_addy() -> ErrStr<()> {
      let toks = fetch_token_registry(&AVALANCHE).await?;
      let btc_mb_addy = toks.token("btc")?.address;
      assert!(btc_mb_addy.is_some());
      btc_mb_addy.and_then(|btc_addy| {
         assert!(btc_addy.starts_with("0x"));
         Some(())
      });
      Ok(())
   }

    #[tokio::test] async fn test_fetch_token_registry_has_btc_undead_avax()
          -> ErrStr<()> {
        let registry = fetch_token_registry(&AVALANCHE).await?;
        for symbol in ["BTC", "UNDEAD", "AVAX"] {
            assert!(registry.token(symbol).is_ok(),
                    "missing '{symbol}' in avalanche.toml");
        }
        Ok(())
    }
}
