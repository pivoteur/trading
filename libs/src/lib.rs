#![crate_name = "libs"]

/// Types used across the library
pub mod types;

/// Collections specific to Pivot Protocol dapps
pub mod collections;

/// Parsing row-data to tables
pub mod tables;

/// Resolves the paths of the pivot-pools
pub mod paths;

/// Fetch data from REST endpoints
pub mod fetchers;

/// processors for pools/proposals
pub mod processors;

/// reports, ... for when you want to report stuff
pub mod reports;

/// Live trading: wallet balances, KyberSwap quotes, keystore signing,
/// swap execution, and plain ERC-20 transfers.
pub mod auto_trading;
