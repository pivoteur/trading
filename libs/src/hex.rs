pub fn pad_address_for_call(address: &str) -> String {
    format!("{:0>64}", address.trim_start_matches("0x").to_lowercase())
}
