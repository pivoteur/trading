//============================================================================
//----- Shared Trading Constants -----------------------------------------------
//============================================================================

pub const DUST_EPSILON: f32 = 1e-8;
pub const UNDEAD: &'static str = "UNDEAD";

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
pub mod test_wallets {
   pub const TEST_ADDRESS: &'static str =
      "0x70D0dF26F6A61fC33ef28EB490b9A645bCb3753A";
}
