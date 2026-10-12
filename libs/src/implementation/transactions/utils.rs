use ethers::{
   middleware::{ SignerMiddleware, Middleware },
   providers::{ Http, Provider },
   signers::{ LocalWallet, Signer },
   types::{
      Address, Bytes, Eip1559TransactionRequest, U256,
      transaction::eip2718::TypedTransaction
   }
};

use book::{ debug, err_utils::{ ErrStr, err_or } };
use libs::types::blockchains::Blockchain;

pub fn f32_2_u128(amount: f32, decimals: i32) -> u128 {
   (amount * 10f32.powi(decimals)).round() as u128
}
pub fn u128_2_f32(amount_base: u128, decimals: i32) -> f32 {
   amount_base as f32 / 10f32.powi(decimals)
}

pub async fn load_signer(blockchain: &Blockchain, expected_address: &str,
                         keystore_path: &str) -> ErrStr<LocalWallet> {
    let password = match std::env::var("KEYSTORE_PASSWORD") {
        Ok(pw) => pw,
        Err(_) => err_or(rpassword::prompt_password("Keystore password: "),
                         "Could not read password. No funds moved.")?
    };  
    let wallet =
       err_or(LocalWallet::decrypt_keystore(&keystore_path, &password),
              &format!("Could not decrypt keystore, path {keystore_path}.
No funds moved."))?
             .with_chain_id(blockchain.chain_id());
    let derived = format!("{:?}", wallet.address());
    if !derived.eq_ignore_ascii_case(expected_address) {
        Err(format!("Keystore address ({derived}) does not match expected
address ({expected_address}) — refusing to proceed. No funds moved."
        ))
    } else { 
       Ok(wallet)        
    }
}  

/// Builds an EIP-1559 tx with a buffered max fee (so a base-fee bump between
/// estimation and submission — e.g. during the password prompt — doesn't
/// get the tx rejected pre-mempool) and a buffered gas limit. Fees are
/// re-estimated fresh on every call rather than reused across steps.
pub async fn build_tx_with_fee_buffer(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>, to: Address,
        data: Bytes, mb_value: Option<U256>)
       -> ErrStr<Eip1559TransactionRequest> {
    let (max_fee, max_priority_fee) =
       err_or(client.estimate_eip1559_fees(None).await,
              "Could not estimate EIP-1559 fees")?;
    // 30% buffer on the max fee absorbs a base-fee bump between estimation
    // and submission without overpaying on the priority fee.
    let buffered_max_fee =
       max_fee.saturating_mul(U256::from(130)) / U256::from(100);
    let mut tx = Eip1559TransactionRequest::new()
        .to(to)
        .data(data)
        .max_fee_per_gas(buffered_max_fee)
        .max_priority_fee_per_gas(max_priority_fee);
    mb_value.and_then(|value| { tx = tx.clone().value(value); Some(()) });
    let typed: TypedTransaction = tx.clone().into();
    let gas_estimate = err_or(client.estimate_gas(&typed, None).await,
                              "Could not estimate gas limit")?;
    // 20% buffer on gas so slightly-off estimate doesn't run out mid-execution.
    let buffered_gas =
       gas_estimate.saturating_mul(U256::from(120)) / U256::from(100);
    Ok(tx.gas(buffered_gas))
}

pub async fn complete_transaction(
        client: &SignerMiddleware<Provider<Http>, LocalWallet>,
        tx: Eip1559TransactionRequest, tx_type: &str, check_state: bool,
        verbose: bool) -> ErrStr<(String, f32)> {
    debug!("complete_transaction", verbose);
    let pending = err_or(client.send_transaction(tx, None).await,
                         &format!("{tx_type} transaction failed to send"))?;
    let tx_hash = format!("{:?}", pending.tx_hash());
    log!("Transaction {} tx submitted: {}", tx_type, tx_hash);
    
    let receipt =
       err_or(pending.await,
              &format!("{tx_type} transaction failed while confirming"))?;
    match receipt {
        Some(r) => {
            if !check_state || r.status == Some(1.into()) {
               log!("Transaction {} confirmed in block {}",
                    tx_type, format!("{:?}", r.block_number));
               Ok((tx_hash, gas_cost_avax(r.gas_used, r.effective_gas_price)))
            } else {
               Err(format!("{tx_type} transaction REVERTED on-chain.
Hash: {tx_hash}"))
            }
        }
        None => Err(format!("{tx_type} transaction was dropped or replaced.
Hash: {tx_hash}"))
    }
}

/// AVAX cost of a confirmed transaction, computed from its own receipt
/// (gas_used * effective_gas_price), not estimated beforehand. If a
/// receipt is somehow missing pricing info, returns 0.0 rather than
/// failing the whole trade over a cosmetic figure — the trade itself
/// already succeeded by the time this is called.
fn gas_cost_avax(gas_used: Option<U256>, gas_price: Option<U256>) -> f32 {
    match (gas_used, gas_price) {
        (Some(g), Some(p)) => {
            let wei = g.saturating_mul(p);
            wei.as_u128() as f32 / 1e18
        }
        _ => 0.0,
    }
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod test_data {
   pub const ZERONE: u128 = 100000007020609536;
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use super::test_data::ZERONE;
   use paste::paste;
   use book::create_testing;

   create_testing!("implementation::transactions::utils");

   run!("f32_2_u128", {
      let ans = f32_2_u128(0.1, 18);
      println!("f32 0.1 as u128 is {ans}");
   });

   run!("u128_2_f32", {
      let ans = u128_2_f32(ZERONE, 18);
      println!("u128 {ZERONE} as f32 is {ans}");
   });
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;
   use book::num::estimate::mk_estimate;

   #[test] fn test_f32_fns_idempotent() {
      let one = 1.0;
      let x = f32_2_u128(one, 18);
      let x1 = u128_2_f32(x, 18);
      let oneish = mk_estimate(x1);
      oneish.approximates(one);
   }
}
