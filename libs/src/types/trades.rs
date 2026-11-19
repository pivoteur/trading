#[derive(Debug, Clone)]
pub enum Trade {
   SIMULATED,
   EXECUTED {
      pub tx_hash: String,
      pub actual_received: f32,
#[allow(dead_code)]
      gas_avax: f32
   }
}
