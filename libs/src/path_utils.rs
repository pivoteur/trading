use libs::types::blockchains::Blockchain;

use super::git_resources::trading_data_dir;

pub fn token_url(blockchain: Blockchain) -> String {
   format!("{}/tokens/{}.toml", trading_data_dir(), blockchain.blockchain())
}
