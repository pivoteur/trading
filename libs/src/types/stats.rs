use book::num::percentage::{ Percentage, mk_percentage };

#[derive(Debug, Clone, Default)]
pub struct CumulativeStats {
    pub total_opens:       usize,
    pub total_closes:      usize,
    pub total_gain_asset:  f64,
    pub total_gain_undead: f64,
    pub total_gas_avax:    f64,
    pub roi_sum: f64,
    pub apr_sum: f64
}   

fn zero_or(num: f64, dem: usize) -> Percentage {
   let n = num as f32;
   mk_percentage(if dem == 0 { 0.0 } else { n / dem as f32 })
}

impl CumulativeStats {
   pub fn open_pivots(&self) -> usize { self.total_opens - self.total_closes }
   pub fn avg_roi(&self) -> Percentage {
      zero_or(self.roi_sum, self.total_closes)
   }
   pub fn avg_apr(&self) -> Percentage {
      zero_or(self.apr_sum, self.total_closes)
   }

   pub fn report(&self) -> String {
format!("
  pool roi:                 {}
  pool apr:                 {}
  total profit, UNDEAD:     {:+.8}
  total profit, BTC:        {:+.4}
  total gas used:           {:.5} AVAX
", self.avg_roi(), self.avg_apr(), self.total_gain_undead,
   self.total_gain_asset, self.total_gas_avax)
   }
   pub fn mb_warning(&self) {
      if self.total_gain_asset < 0.0 || self.total_gain_undead < 0.0 {
         println!("
  \u{26A0} WARNING: realized cumulative gain is negative

      — this is NOT a timing artifact,

closes are actually losing money. BTC {:+.4}   UNDEAD {:+.8}",
         self.total_gain_asset, self.total_gain_undead);
      }
   }
}

