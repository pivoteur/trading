#[derive(Debug, Clone)]
pub struct BalanceSnapshot {
    pub asset_balance:    f64,
    pub asset_committed:  f64,
    pub asset_available:  f64,
    pub undead_balance:   f64,
    pub undead_committed: f64,
    pub undead_available: f64
}

fn asset_status(symbol: &str, in_wallet: f64, committed: f64, available: f64) 
      -> String {
   format!("{symbol} {in_wallet:.4} in wallet 
        ({committed:.4} committed, {available:.4})")
}

impl BalanceSnapshot {
   pub fn status(&self) -> String {
      format!("
{}
{}", asset_status("BTC", self.asset_balance, self.asset_committed,
                  self.asset_available),
     asset_status("UNDEAD", self.undead_balance, self.undead_committed,
                  self.undead_available))
   }
}
