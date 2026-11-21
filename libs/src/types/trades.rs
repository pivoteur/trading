#[derive(Debug, Clone)]
pub enum Trade {
   SIMULATED,
   EXECUTED {
      tx_hash: String,
      actual_received: f32,
      gas_avax: f32
   }
}
