use book::{ currency::usd::USD, err_utils::ErrStr };
use super::balances::tokens::TokenBalance;

pub trait Wallet {
   fn quote(&self, token: &str) -> impl Future<Output = ErrStr<USD>>;
   fn balances(&self) -> impl Future<Output = ErrStr<Vec<TokenBalance>>>;
   fn send(&self) -> impl Future<Output = ErrStr<()>>;
   fn trade(&self) -> impl Future<Output = ErrStr<()>>;
}

// Each implementation is responsible for their own constructor
