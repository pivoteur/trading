#[derive(Debug, Clone, Default)]
pub struct CumulativeStats {
    pub total_opens:       usize,
    pub total_closes:      usize,
    pub total_gain_asset:  f64,
    pub total_gain_undead: f64,
    pub total_gas_avax:    f64,
    pub roi_sum: f64,
    pub apr_sum: f64,
}   

fn zero_or(num: f64, dem: usize) -> f64 {
   if dem == 0 { 0.0 } else { num / dem as f64 }
}

impl CumulativeStats {
   pub fn avg_roi(&self) -> f64 { zero_or(self.roi_sum, self.total_closes) }
   pub fn avg_apr(&self) -> f64 { zero_or(self.apr_sum, self.total_closes) }
}                     

