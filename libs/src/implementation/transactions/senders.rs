use std::str::FromStr;

use ethers::{
   middleware::SignerMiddleware,
   providers::{ Http, Provider },
   types::{ Address, Bytes, U256, H160 }
};

use book::{
   debug,
   err_utils::{ ErrStr, err_or },
   string_utils::s,
   utils::pred
};
use libs::types::blockchains::Blockchain;
use super::utils::{
   build_tx_with_fee_buffer,
   complete_transaction,
   f32_2_u128
};
use crate::{
   implementation::{
      consts::DUST_EPSILON,
      hex::{ pad_address_for_call, pad_u256_for_call },
      transactions::utils::load_signer
   },
   types::tokens::{ TokenRegistry, TokenEntry }
};

/// Transfer core behind `send_tokens_to_address` — a plain ERC-20
/// transfer(), not a swap. Same safety baseline as execute_trade: verified
/// signer, EIP-1559 fee buffering, hard error on revert/drop/failure
/// rather than a false success. Does not support native AVAX (no
/// tokens.toml address to encode against) — only ERC-20s with a real
/// contract address. `to_address` is always a literal here — nothing in
/// this file resolves an address from env internally anymore.
pub async fn send_tokens_to_address(blockchain: &Blockchain,
                                    addy: &str, registry: &TokenRegistry,
                                    symbol: &str, to_address: &str,
                                    amount: f32, keystore_path: &str,
                                    debug: bool) -> ErrStr<(String, f32)> {
   debug!("send_token_to_address", debug);
   check_eps(amount)?;
   let signer = load_signer(blockchain, addy, keystore_path).await?;
   let provider = err_or(Provider::<Http>::try_from(&blockchain.url()),
                         "Could not create RPC provider")?;
   let client = SignerMiddleware::new(provider, signer);
   let (entry, mb_token_addr) = check_registry(symbol, registry)?;
   let amount_base = f32_2_u128(amount, entry.decimals as i32);
   let (to, data, value) =
      build_send_tx(entry.native, mb_token_addr,
                    to_address, amount_base, debug)?;
   log!("Parameters for sending: ({})",
        format!("{}, {}, {:?}", to, data, value));

   // Update your builder function to accept the optional 'value' field. If
   // your existing function can't be modified, see the adjustment note below.
   let tx = build_tx_with_fee_buffer(&client, to, data, value).await?;

   log!("On its way — courier's en route...");
   complete_transaction(&client, tx, "transfer", true, debug).await
}

fn check_eps(amount: f32) -> ErrStr<()> {
   pred(amount > DUST_EPSILON, ())
      .ok_or(format!("amount must be more than {DUST_EPSILON}, got {amount}"))
}

fn check_registry(symbol: &str, registry: &TokenRegistry)
      -> ErrStr<(TokenEntry, Option<String>)> {
   let entry = registry.token(symbol)?;
   let mb_addr = &entry.address.clone();
   Ok((entry, mb_addr.clone()))
}           

fn build_send_tx(native: bool, mb_token_addr: Option<String>, 
                 to_address: &str, amount_base: u128, debug: bool)
      -> ErrStr<(Address, Bytes, Option<U256>)> {
   if native {
      build_send_native(to_address, amount_base, debug)
         .and_then(|(a,b,c)| Ok((a, b, Some(c))))
   } else {
      build_send_erc20(mb_token_addr, to_address, amount_base, debug)
         .and_then(|(a,b)| Ok((a, b, None)))
   }
}

fn build_send_native(to_address: &str, amount_base: u128, debug: bool)
      -> ErrStr<(H160, Bytes, U256)> {
   debug!("build_send_native", debug);
   log!("branch");
   // Native Transfer: Target is the actual recipient, payload is empty,
   // value holds the amount
   let to = err_or(Address::from_str(to_address), "Bad recipient address")?;
   let data = Bytes::new(); // Empty calldata
   let value = U256::from(amount_base);
   Ok((to, data, value))
}
    
fn build_send_erc20(mb_token_addr: Option<String>, to_address: &str,
                    amount_base: u128, debug: bool) -> ErrStr<(H160, Bytes)> {
   debug!("build_send_erc20", debug);
   log!("branch");
   let token_addr = mb_token_addr.ok_or(s("No token address"))?;
   
   // ERC-20 Transfer: Target is the token contract, payload contains
   // encoded call, value is 0
   let data_hex =   
      format!("0xa9059cbb{}{}",
              pad_address_for_call(to_address),
              pad_u256_for_call(amount_base));
   let to = err_or(Address::from_str(&token_addr), "Bad token address")?;
   let data = err_or(Bytes::from_str(&data_hex), "Bad transfer calldata")?;
   Ok((to, data))
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use crate::implementation::{
      consts::DUST_EPSILON,
      fetchers::tokens::fetch_token_registry
   };
   use libs::types::blockchains::Blockchain::*;

   #[test] fn fail_check_eps() {
      let amt = DUST_EPSILON / 2.0;
      let ans = check_eps(amt);
      assert!(ans.is_err(), "Allowing tiny sends");
   }

   #[test] fn test_check_eps_ok() {
      let amt = check_eps(1.0);
      assert!(amt.is_ok());
   }

   #[tokio::test] async fn fail_check_registry_avalanche() -> ErrStr<()> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let chk = check_registry("trump", &registry);
      assert!(chk.is_err(), "No Kings");
      Ok(())
   }

   #[tokio::test] async fn fail_check_registry_binance() -> ErrStr<()> {
      let registry = fetch_token_registry(&BINANCE).await?;
      let chk = check_registry("luna", &registry);
      assert!(chk.is_err(), "Funnily enough; both are of COSMOS-origin");
      Ok(())
   }

   #[tokio::test] async fn test_check_registry_avalanche_protocol_ok() 
         -> ErrStr<()> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let chk = check_registry("avax", &registry);
      assert!(chk.is_ok(), "Avalanche registry check, protocol token");
      Ok(())
   }

   #[tokio::test] async fn test_check_registry_binance_protocol_ok()
         -> ErrStr<()> {
      let registry = fetch_token_registry(&BINANCE).await?;
      let chk = check_registry("bnb", &registry);
      assert!(chk.is_ok(), "Binance registry check, protocol token");
      Ok(())
   }

   #[tokio::test] async fn test_check_registry_avalanche_erc20_ok()
         -> ErrStr<()> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let chk = check_registry("btc", &registry);
      assert!(chk.is_ok(), "Avalanche registry check");
      Ok(())
   }

   #[tokio::test] async fn test_check_registry_erc20_protocol_ok()
         -> ErrStr<()> {
      let registry = fetch_token_registry(&BINANCE).await?;
      let chk = check_registry("eth", &registry);
      assert!(chk.is_ok(), "Binance registry check");
      Ok(())
   }

   const SOME_ADDRESS: &'static str =
      "0x0123456789abcdef0123456789abcdef01234567";

   #[test] fn fail_build_send_tx() {
      let ans =
         build_send_tx(false, None, SOME_ADDRESS, f32_2_u128(1.0, 18), true);
      assert!(ans.is_err(), "Non-native token without contract address");
   }

   #[test] fn fail_build_send_tx_native_bad_to_address() {
      let ans =
         build_send_tx(true, None, "0xabc", f32_2_u128(1.0, 18), true);
      assert!(ans.is_err(), "Should not form tx with bad address {ans:?}");
   }

   #[test] fn test_build_send_tx_native() {
      let ans =
         build_send_tx(true, None, SOME_ADDRESS, f32_2_u128(1.0, 18), true);
      assert!(ans.is_ok(), "tx should succeed for protocol token {ans:?}");
   }

   #[test] fn fail_build_send_tx_bad_token_address() {
      let ans =
	 build_send_tx(false, Some(s("0x123abcd")), SOME_ADDRESS,
                       f32_2_u128(1.0, 18), true);
      assert!(ans.is_err(), "tx should succeed for some token 'xyz' {ans:?}");
   }

   #[tokio::test] async fn test_build_send_tx_btc_avalanche() -> ErrStr<()> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let (entry, mb_token_addr) = check_registry("ETH", &registry)?;
      assert!(!entry.native);
      let ans = build_send_tx(entry.native, mb_token_addr, SOME_ADDRESS,
                              f32_2_u128(0.1, entry.decimals as i32), true);
      assert!(ans.is_ok(), "tx should build for Avalanche BTC {ans:?}");
      Ok(())
   }

   #[tokio::test] async fn test_build_send_tx_doge_binance() -> ErrStr<()> {
      let registry = fetch_token_registry(&BINANCE).await?;
      let (entry, mb_token_addr) = check_registry("doge", &registry)?;
      assert!(!entry.native);
      let ans = build_send_tx(entry.native, mb_token_addr, SOME_ADDRESS,
                              f32_2_u128(0.1, entry.decimals as i32), true);
      assert!(ans.is_ok(), "tx should build for Binance DOGE {ans:?}");
      Ok(())
   }

   #[tokio::test] async fn test_build_send_tx_native_binance_with_token_addy()
         -> ErrStr<()> {
      let registry = fetch_token_registry(&BINANCE).await?;
      let (entry, mb_token_addr) = check_registry("bnb", &registry)?;
      assert!(entry.native);
      assert!(mb_token_addr.is_some());
      let ans = build_send_tx(entry.native, mb_token_addr, SOME_ADDRESS,
                              f32_2_u128(1.0, entry.decimals as i32), true);
      assert!(ans.is_ok(), "tx should build for Binance BNB {ans:?}");
      Ok(())
   }

   #[tokio::test] async fn test_build_send_tx_native_avalanche_with_token_addy()
         -> ErrStr<()> {
      let registry = fetch_token_registry(&AVALANCHE).await?;
      let (entry, mb_token_addr) = check_registry("avax", &registry)?;
      assert!(entry.native);
      assert!(mb_token_addr.is_some());
      let ans = build_send_tx(entry.native, mb_token_addr, SOME_ADDRESS,
                              f32_2_u128(1.0, entry.decimals as i32), true);
      assert!(ans.is_ok(), "tx should build for Avalanche AVAX {ans:?}");
      Ok(())
   }
}
