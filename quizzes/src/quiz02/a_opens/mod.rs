use chrono::NaiveDate;

use clap::Parser;

use book::{
   parse_args_add_banner,
   cli_utils::generate_banner,
   csv_utils::list_csv,
   err_utils::ErrStr,
   file_utils::lines_from_file,
   num::floats::comma_floats::CommaFloat,
   string_utils::UppercaseString
};
use libs::{
   fetchers::{ pivots::parse_pivots, quotes::fetch_quotes },
   types::{ pivots::opens::pivot_assets, pools::compute_pool, quotes::Quotes }
};

#[derive(Debug, Parser)]
#[command(name="opens")]
#[command(version="0.1.1")]
struct Args {

   /// token to pivot
   token: UppercaseString,

   /// Amount to pivot
   amount: CommaFloat,

   /// Pivot-token
   pivot: UppercaseString,

   /// Path to open pivots
   path: String,

   /// Date to analyze available assets
   date: NaiveDate,

   /// Show debugging information
   #[arg(long, short, default_value_t=false)]
   debug: bool
}

pub async fn runoff_with_args() -> ErrStr<()> {
   let args = parse_args_add_banner!(Args);
   let quotes = fetch_quotes(&args.date).await?;
   runoff_continuation(&quotes, &args.path,
                       &args.token, args.amount.into(), &args.pivot, args.debug)
}

fn runoff_continuation(quotes: &Quotes, path: &str,
                       primary: &str, _amount: f32, pivot: &str, debug: bool)
      -> ErrStr<()> {
   let pool = compute_pool(quotes, primary, pivot, debug)?;
   let file = lines_from_file(path)?;
   let aliases = &quotes.aliases;
   let ((opens, _closes), _dt) = parse_pivots(&pool, file, aliases, debug)?;
   println!("Committed assets:

{}", list_csv(&pivot_assets(&opens)?.assets()));
   Ok(())
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::create_testing;
   use libs::types::quotes::sample_data::sample_btc_eth_quotes;

   create_testing!("quiz02::a_opens");

   run!("opens", {
      let mut qts = sample_btc_eth_quotes();
      runoff_continuation(&mut qts, "data/pivots/open/raw/btc-eth.tsv", "BTC", 
                          1.0, "ETH", true)?;
   });
}
