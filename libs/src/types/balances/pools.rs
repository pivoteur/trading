#[derive(Debug, Clone)]
pub struct BalanceSnapshot {
    asset_balance:    f32,
    asset_committed:  f32,
    undead_balance:   f32,
    undead_committed: f32
}

pub fn mk_balance_snapshot(assbal: Option<f32>, asset_committed: f32,
                           undbal: Option<f32>, undead_committed: f32)
      -> BalanceSnapshot {
   let asset_balance = assbal.unwrap_or(0.0);
   let undead_balance = undbal.unwrap_or(0.0);
   BalanceSnapshot { asset_balance, asset_committed,
                     undead_balance, undead_committed }
}

fn asset_status(symbol: &str, in_wallet: f32, committed: f32) -> String {
   format!("{symbol} {in_wallet:.4} in wallet 
        ({committed:.4} committed, {:.4} available)", in_wallet - committed)
}

impl BalanceSnapshot {
   pub fn status(&self) -> String {
      format!("
{}
{}", asset_status("BTC", self.asset_balance, self.asset_committed),
     asset_status("UNDEAD", self.undead_balance, self.undead_committed))
   }
}
