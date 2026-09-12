use book::err_utils::{ ErrStr, err_or };

pub fn pad_address_for_call(address: &str) -> String {
    format!("{:0>64}", address.trim_start_matches("0x").to_lowercase())
}

pub fn hex_to_u128(hex: &str) -> ErrStr<u128> {
    let trimmed0 = hex.trim_start_matches("0x");
    let trimmed = if trimmed0.is_empty() { "0" } else { trimmed0 };
    err_or(u128::from_str_radix(trimmed, 16),
           &format!("Could not parse hex balance '{hex}'"))
}

// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;

    #[test] fn test_pad_address_for_call_produces_32_byte_word() {
        let padded =
           pad_address_for_call("0x69b21DC480CA62E478D997d7313061F765a5B122");
        assert_eq!(padded.len(), 64);
        assert!(padded.ends_with("b122"));
        assert!(padded.starts_with("00000000000000000000"));
    }

    #[test] fn test_hex_to_u128_parses_rpc_style_hex() -> ErrStr<()> {
        assert_eq!(hex_to_u128("0x0")?, 0);
        assert_eq!(hex_to_u128("0x")?, 0);
        assert_eq!(hex_to_u128("0xff")?, 255);
        assert_eq!(hex_to_u128("0xde0b6b3a7640000")?,
                   1_000_000_000_000_000_000);
        Ok(())
    }

    #[test] fn fail_hex_to_u128_garbage() {
        assert!(hex_to_u128("0xnotarealnumber").is_err());
    }
}
