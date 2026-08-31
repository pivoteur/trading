use book::{ debug, not_implemented, err_utils::ErrStr };
use libs::{
   types::{ blockchains::Blockchain, pools::Pool }
};

use super::{
/*
   auto_trading::{
      query_swap,
   },
*/
   path_utils::trade_log_path
};

pub async fn create_log(path: &str, wallet: &str, blockchain: &Blockchain,
                        pool: &Pool, amt: f64, dry_run: bool, debug: bool)
      -> ErrStr<String> {
   debug!("libs::log_utils::create_log", debug);
   let file_name = trade_log_path(path, wallet, blockchain, pool, debug);
   not_implemented!("create_log", file_name, amt, dry_run)
}

/*
pub async fn query_swap(
    blockchain: &str,
    registry: &TokenRegistry,
    from_symbol: &str,
    to_symbol: &str,
    amount: f64,
    debug: bool 
 */

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use libs::types::{ blockchains::Blockchain::AVALANCHE,pools::pool_from_str };
   // use book::create_testing;

   #[tokio::test] async fn test_create_log_ok() -> ErrStr<()> {
      let pool = pool_from_str("btc-undead")?;
      let mb_file_name =
         create_log("xyz", "0x123", &AVALANCHE, &pool, 0.01, true, true).await;
      assert!(mb_file_name.is_ok());
      Ok(())
   }
}
