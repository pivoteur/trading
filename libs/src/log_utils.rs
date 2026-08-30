use book::{ debug, not_implemented, err_utils::ErrStr };
use libs::{
   fetchers::quotes::fetch_quotes,
   types::{ blockchains::Blockchain, pools::Pool }
};

// use super::auto_trading::{
   // query_swap,

pub async create_log(path: &str, wallet: &str, blockchain: &Blockchain,
                     primary: &str, amt: f64, pivot: &str,
                     dry_run: bool, debug: bool) -> ErrStr<()> {
   debug!("libs::log_utils::create_log");
   not_implemented!("create_log)
}

fn pool_file_name(wallet: &str, p: &Pool, 

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
   use book::create_testing;

   #[tokio::test] fn test_create_log_ok() -> ErrStr<()> {

