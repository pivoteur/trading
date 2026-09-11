#[derive(Debug, Clone)]
pub struct BalanceSnapshot {
    pub asset_balance:    f64,
    pub asset_committed:  f64,
    pub asset_available:  f64,
    pub undead_balance:   f64,
    pub undead_committed: f64,
    pub undead_available: f64
}

