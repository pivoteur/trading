/// `0x` followed by exactly 40 hex characters -- no checksum validation,
/// just enough of a shape check to catch a fat-fingered or truncated
/// address before it gets baked into ERC-20 transfer calldata, where a
/// malformed address would otherwise fail silently (or worse, resolve to
/// some other real address) instead of erroring up front.
pub fn is_valid_evm_address(address: &str) -> bool {
   address.strip_prefix("0x")
          .and_then(|hex|
        Some(hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())))
          .unwrap_or(false)
}


// ----- TESTS -------------------------------------------------------

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
   use super::*;

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
}
