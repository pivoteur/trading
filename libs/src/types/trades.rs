#[derive(Debug, Clone)]
pub enum Trade {
   SIMULATED,
   EXECUTED {
#[allow(dead_code)]
      tx_hash: String,
#[allow(dead_code)]
      actual_received: f32,
#[allow(dead_code)]
      gas_avax: f32
   }
}
