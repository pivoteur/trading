use book::{ currency::usd::USD, err_utils::ErrStr };
use super::balances::tokens::TokenBalance;

pub trait Wallet {
   fn quote(&self, token: &str) -> impl Future<Output = ErrStr<USD>> + Send;
   fn balances(&self) -> impl Future<Output = ErrStr<Vec<TokenBalance>>> + Send;
   fn send(&self) -> impl Future<Output = ErrStr<()>> + Send;
   fn trade(&self) -> impl Future<Output = ErrStr<()>> + Send;
}

// Each implementation is responsible for their own constructor
