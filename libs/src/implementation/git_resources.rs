use book::string_utils::s;

fn trading_raw_url() -> String {
   s("https://raw.githubusercontent.com/pivoteur/trading/refs/heads/main")
}

pub fn trading_data_dir() -> String { format!("{}/data", trading_raw_url()) }
