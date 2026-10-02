use book::{ currency::usd::USD, err_utils::ErrStr };

pub trait Wallet {
   fn quote(&self) -> impl Future<Output = ErrStr<USD>> + Send;
   fn balances(&self) -> impl Future<Output = ErrStr<()>> + Send;
   fn send(&self) -> impl Future<Output = ErrStr<()>> + Send;
   fn trade(&self) -> impl Future<Output = ErrStr<()>> + Send;
}

// Each implementation is responsible for their own constructor
