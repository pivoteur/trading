use book::{ debug, not_implemented };
use libs::types::{
   blockchains::Blockchain,
   pools::Pool
};

pub fn trade_log_path(trading_data_dir: &str, wallet: &str,
                      blockchain: &Blockchain, pool: &Pool, debug: bool)
      -> String {
   debug!("trade_log_path", debug);
   not_implemented!("trade_log_path",
                    trading_data_dir, wallet, blockchain, pool, debug);
}

pub fn token_path(trading_data_dir: &str, blockchain: &Blockchain) -> String {
   not_implemented!("token_path", trading_data_dir, blockchain)
}

