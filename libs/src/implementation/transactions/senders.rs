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
use super::utils::{ build_tx_with_fee_buffer, complete_transaction };
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
                                    verbose: bool) -> ErrStr<(String, f32)> {
   debug!("send_token_to_address", verbose);
   check_eps(amount)?;
   let signer = load_signer(blockchain, addy, keystore_path).await?;
   let provider = err_or(Provider::<Http>::try_from(&blockchain.url()),
                         "Could not create RPC provider")?;
   let client = SignerMiddleware::new(provider, signer);
   let (entry, mb_token_addr) = check_registry(symbol, registry)?;
   let amount_base =
       (amount * 10f32.powi(entry.decimals as i32)).round() as u128;
   let (to, data, value) = if entry.native {
      build_send_native(to_address, amount_base, verbose)
         .and_then(|(a,b,c)| Ok((a, b, Some(c))))?
   } else {
      build_send_erc20(mb_token_addr, to_address, amount_base, verbose)
         .and_then(|(a,b)| Ok((a, b, None)))?
   };
   log!("Parameters for sending: ({})",
        format!("{}, {}, {:?}", to, data, value));

   // Update your builder function to accept the optional 'value' field. If
   // your existing function can't be modified, see the adjustment note below.
   let tx = build_tx_with_fee_buffer(&client, to, data, value).await?;

   log!("On its way — courier's en route...");
   complete_transaction(&client, tx, "transfer", true, verbose).await
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

fn build_send_native(to_address: &str, amount_base: u128, verbose: bool)
      -> ErrStr<(H160, Bytes, U256)> {
   debug!("build_send_native", verbose);
   log!("branch");
   // Native Transfer: Target is the actual recipient, payload is empty,
   // value holds the amount
   let to = err_or(Address::from_str(to_address), "Bad recipient address")?;
   let data = Bytes::new(); // Empty calldata
   let value = U256::from(amount_base);
   Ok((to, data, value))
}
    
fn build_send_erc20(mb_token_addr: Option<String>, to_address: &str,
                    amount_base: u128, verbose: bool) -> ErrStr<(H160, Bytes)> {
   debug!("build_send_erc20", verbose);
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
