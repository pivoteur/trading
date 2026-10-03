use serde_json::Value;

/// A live quote plus everything needed to actually build and sign the swap
/// afterward.
#[derive(Debug)]
pub struct KyberSwap {
    pub amount_out:         f32,
    pub route_summary_raw:  Value,
    pub router_address:     String
}
