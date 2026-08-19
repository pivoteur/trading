use clap::Parser;
use book::{
        debug,
        err_utils::ErrStr,
        file_utils::read_file,
        parse_args_add_banner,
        string_utils::UppercaseString,
        cli_utils::generate_banner,
    };
use trading::auto_trading::{
                TokenRegistry,
                parse_token_registry,
                token_entry,
                wallet_balance,
                send_tokens_to_address,
                now_ts,
                log_ts,
                append_trade_log_line,
};


//============================================================================
// ----- const ----------------------------------------------------------------
//============================================================================
const DUST_EPSILON: f64 = 1e-8;
const DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
const TRADE_LOG_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/sendan-sends.log");
const TRADE_LOG_HEADER: &str = "timestamp\tmode\tblockchain\ttoken\tamount\tto_address\toutcome\tactual_sent\tgas_avax\ttx_hash";
const KEYSTORE_PATH_VAR: &str = "SENDAN_KEYSTORE_PATH";

pub fn load_token_registry(tokens: &str) -> ErrStr<TokenRegistry> {
    parse_token_registry(tokens)
}

/// `0x` followed by exactly 40 hex characters -- no checksum validation,
/// just enough of a shape check to catch a fat-fingered or truncated
/// address before it gets baked into ERC-20 transfer calldata, where a
/// malformed address would otherwise fail silently (or worse, resolve to
/// some other real address) instead of erroring up front.
fn is_valid_evm_address(address: &str) -> bool {
    match address.strip_prefix("0x") {
        Some(hex) => hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}
//=========================================================================================
// ----- CLI --------------------------------------------------------------------------------
//=========================================================================================
/// sendan (Old English: "to send") -- a one-shot ERC-20 transfer, e.g.
/// `sendan avalanche 1100 UNDEAD 0x12345...`. No pivots, no replayed
/// state: every invocation is a single, independent send.
#[derive(Debug, Parser)]
#[command(name = "sendan", version = "0.1.0")]
struct Args {
    blockchain: String,
    amount: f64,
    token: UppercaseString,
    to_address: String,
    #[arg(long, env = "VAULT_ADDRESS")]
    wallet_address: String,
    #[arg(long, env = "VAULT_KEYSTORE_PATH")]
    keystore_path: String,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(long, default_value_t = false)]
    debug: bool,
}

#[allow(clippy::too_many_arguments)]
fn log_send(
    mode: &str, blockchain: &str, token: &str, amount: f64, to_address: &str,
    outcome: &str, actual_sent: f64, gas_avax: f64, tx_hash: &str,
) {
    let line = format!(
        "{}\t{mode}\t{blockchain}\t{token}\t{amount:.8}\t{to_address}\t{outcome}\t{actual_sent:.8}\t{gas_avax:.8}\t{tx_hash}",
        log_ts(now_ts())
    );
    append_trade_log_line(TRADE_LOG_PATH, &line, Some(TRADE_LOG_HEADER));
}

//========================================================================================
// ----- SEND FN ---------------------------------------------------------------------------
//========================================================================================
#[allow(clippy::too_many_arguments)]
async fn sendan_continuation(
    blockchain: &str, amount: f64, token: &str, to_address: &str,
    wallet_address: &str, keystore_path: &str, dry_run: bool, debug: bool,
) -> ErrStr<()> {
    debug!("sendan_continuation", debug);
    let mode = if dry_run { "DRY-RUN" } else { "LIVE" };
    println!("mode {mode} send {amount:.8} {token} -> {to_address}");

    if amount <= DUST_EPSILON {
        return Err(format!("sendan: amount must be positive, got {amount}"));
    }
    if !is_valid_evm_address(to_address) {
        return Err(format!(
            "'{to_address}' doesn't look like an EVM address -- expected '0x' followed by 40 hex characters."
        ));
    }

    let tokens = read_file(&format!("{DATA_DIR}/{blockchain}.toml"))?;
    let registry = load_token_registry(&tokens)?;

    // fail fast on an unknown/native token before spending an RPC call on
    // a balance check we already know can't lead anywhere.
    let entry = token_entry(&registry, token)?;
    if entry.address.is_none() {
        return Err(format!(
            "'{token}' has no address in data/{blockchain}.toml -- sendan only sends ERC-20s, not the native coin."
        ));
    }

    let balance = wallet_balance(wallet_address, token, &registry).await?;
    println!("wallet {wallet_address}");
    println!("{token} balance {balance:.8}");

    if amount > balance + DUST_EPSILON {
        return Err(format!(
            "insufficient {token} balance: have {balance:.8}, asked to send {amount:.8} -- nothing attempted."
        ));
    }

    if dry_run {
        println!("  WOULD SEND  {amount:.8} {token} -> {to_address}");
        log_send(mode, blockchain, token, amount, to_address, "WOULD_SEND", amount, 0.0, "");
        return Ok(());
    }

    unsafe { std::env::set_var(KEYSTORE_PATH_VAR, keystore_path); }

    match send_tokens_to_address(
        wallet_address, &registry, token, to_address, amount, KEYSTORE_PATH_VAR, debug,
    ).await {
        Ok((tx_hash, gas_avax)) => {
            println!("  SENT  {amount:.8} {token} -> {to_address}   gas {gas_avax:.5} AVAX   tx {tx_hash}");
            log_send(mode, blockchain, token, amount, to_address, "SENT", amount, gas_avax, &tx_hash);
            Ok(())
        }
        Err(e) => {
            println!("  ! send failed, no funds moved: {e}");
            log_send(mode, blockchain, token, amount, to_address, &format!("FAILED: {e}"), 0.0, 0.0, "");
            Err(e)
        }
    }
}

pub async fn runoff_with_args() -> ErrStr<()> {
    let args = parse_args_add_banner!(Args);
    sendan_continuation(
        &args.blockchain, args.amount, &args.token, &args.to_address,
        &args.wallet_address, &args.keystore_path, args.dry_run, args.debug,
    ).await
}

//==========================================================================================
// ----- UNIT TESTS --------------------------------------------------------------------------
//==========================================================================================
#[cfg(test)]
mod unit_tests {
    use super::*;
    use book::utils::now;

    #[test]
    fn test_load_token_registry_has_undead_avax() -> ErrStr<()> {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        for symbol in ["UNDEAD", "AVAX"] {
            assert!(registry.contains_key(symbol), "missing '{symbol}' in tokens.toml");
        }
        Ok(())
    }

    #[test]
    fn test_valid_evm_address_accepts_well_formed_address() {
        assert!(is_valid_evm_address("0x00000000000000000000000000000000000000AB"));
    }

    #[test]
    fn test_valid_evm_address_rejects_missing_0x() {
        assert!(!is_valid_evm_address("000000000000000000000000000000000000AB"));
    }

    #[test]
    fn test_valid_evm_address_rejects_wrong_length() {
        assert!(!is_valid_evm_address("0x1234567891011131314151617181920"));
    }

    #[test]
    fn test_valid_evm_address_rejects_non_hex_characters() {
        assert!(!is_valid_evm_address("0xZZ00000000000000000000000000000000000A"));
    }

    #[test]
    fn test_sendan_continuation_rejects_zero_amount() {
        let result = now(sendan_continuation(
            "avalanche", 0.0, "UNDEAD", "0x000000000000000000000000000000000000CD", "0x123", "", true, false,
        ));
        assert!(result.is_err(), "a zero amount must never reach the wallet-balance check");
    }

    #[test]
    fn test_sendan_continuation_rejects_malformed_address() {
        let result = now(sendan_continuation(
            "avalanche", 100.0, "UNDEAD", "not-an-address", "0x123", "", true, false,
        ));
        assert!(result.is_err(), "a malformed destination must never reach the wallet-balance check");
    }
}
//============================================================================================
// ----- FUNCTIONAL TEST -----------------------------------------------------------------------
//============================================================================================
#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod functional_tests {
    use super::*;
    use paste::paste;
    use book::{ create_testing, utils::now };
    use std::println;


    create_testing!("quiz01::d_sendan");

    run!("wallet_balance_undead", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance(
            "0x123",
            "UNDEAD",
            &registry,
        ))?;
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
    });

    run!("sendan_dry_run_rejects_bad_address", {
        let result = now(sendan_continuation(
            "avalanche", 1.0, "UNDEAD", "not-an-address", "0x123", "", true, false,
        ));
        assert!(result.is_err());
        println!("sendan is ok");
    });

    run!("sendan_functionality", {
        let tokens = read_file(&format!("{DATA_DIR}/avalanche.toml"))?;
        let registry = load_token_registry(&tokens)?;
        let balance = now(wallet_balance("0x123", "UNDEAD", &registry))?;
        println!("\ttest wallet UNDEAD balance: {balance:.8}");
        if balance <= DUST_EPSILON {
            println!("\ttest wallet holds no UNDEAD -- confirming sendan correctly refuses to send rather than assuming a happy path");
            let result = now(sendan_continuation(
                "avalanche", 1.0, "UNDEAD", "0x000000000000000000000000000000000000AB", "0x123", "", true, false,
            ));
            if result.is_ok() {
                return Err("expected an insufficient-balance error against an empty test wallet, got Ok".to_string());
            }
        } else {
            let amount = balance / 2.0;
            now(sendan_continuation(
                "avalanche", amount, "UNDEAD", "0x000000000000000000000000000000000000AB", "0x123", "", true, false,
            ))?;
            println!("\tdry-run WOULD_SEND {amount:.8} UNDEAD accepted end to end");
        }
        println!("sendan is ok");
    });
}
