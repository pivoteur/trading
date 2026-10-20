use clap::Parser;

use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   err_utils::ErrStr,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::types::blockchains::{ Blockchain, Blockchain::AVALANCHE };
use trading::{
   types::{
      modes::execution::{ Execution, Execution::LIVE },
      wallets::Wallet
   },
   wallets::{ factory::connect_wallet, mock::mock_connection }
};

//=================================================================
// ----- CLI -----------------------------------------------------
//===============================================================

#[derive(Debug, Parser)]
#[command(name = "ceap")]
#[command(version = "1.2.2")]
struct Args {
    /// trading from this token
    from_token: UppercaseString,

    /// amount of `from_token` to trade
    amount: CommaFloat,

    /// trading to this token
    to_token: UppercaseString,

    /// Minimum acceptable output amount.
    #[arg(long, default_value_t = CommaFloat(0.0))]
    floor: CommaFloat,

    /// Slippage percent (where 1000 BPS is 10% higher than floor)
    #[arg(long, default_value_t = 0)]
    slippage_bps: u16,

    /// wallet address on which trade occurs
    #[arg(long, env="WALLET_ADDRESS")]
    wallet_address: String,

    /// Keystore to wallet: permits trades on this wallet
    #[arg(long, env = "KEYSTORE_PATH")]
    keystore_path: String,

    /// blockchain on which to execute trade
    #[arg(long, default_value_t = AVALANCHE)]
    blockchain: Blockchain,

    /// Force a dry run
    #[arg(long, default_value_t = LIVE)]
    dry_run: Execution,

    /// Show debugging information
    #[arg(short, long)]
    debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
   let args = parse_args_add_banner!(Args);
   let amount: f32 = args.amount.into();
   let floor: f32 = args.floor.into();
   let chain = &args.blockchain;
   let debug = args.debug;
   let wallet = if args.dry_run == LIVE {
      connect_wallet(chain, &args.wallet_address,
                     &args.keystore_path, debug).await?
   } else {
      mock_connection(chain, debug).await?
   };
   runoff_continuation(&wallet, &args.from_token, &args.to_token, amount,
                       floor, args.slippage_bps).await
}

async fn runoff_continuation(wallet: &Box<dyn Wallet>, 
                             from: &str, to: &str,
                             amount: f32, floor: f32, slippage: u16)
      -> ErrStr<()> {
   let _ = wallet.trade(from, amount, to, Some(floor), slippage).await?;
   Ok(())
}

// =======================================================================
// ----- FUNCTIONAL TESTS --------------------------------------------------
// =======================================================================
#[cfg(not(tarpaulin_include))]
#[cfg(test)]
pub mod functional_test {
   use super::*;
   use paste::paste;
   use book::{ create_testing, utils::now };
   use trading::wallets::mock::mock_connection;

   create_testing!("quiz01::d_ceap");

   run!("ceap", {
      let ava = &AVALANCHE;
      let wallet = mock_connection(ava, true);
      now(runoff_continuation(&wallet, "BTC", "ETH", 1.0, 16.0, 200))?;
   });
}
