use std::{
   fs::OpenOptions,
   io::Write,
   path::Path,
   time::{ SystemTime, UNIX_EPOCH }
};
use chrono::{ DateTime, Utc };
use book::err_utils::ErrStr;
use libs::types::util::Id;
use super::types::{ balances::BalanceSnapshot, stats::CumulativeStats };

pub const LOG_TS_FORMAT: &'static str = "%Y-%m-%d %H:%M:%S";

fn now_ts() -> u64 {
   SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap()
}

pub fn log_ts() -> String {
    let epoch = now_ts();
    DateTime::<Utc>::from_timestamp(epoch as i64, 0)
        .map(|dt| dt.format(LOG_TS_FORMAT).to_string())
        .unwrap_or_else(|| format!("(bad timestamp: {epoch})"))
}

pub fn parse_log_ts(s: &str) -> ErrStr<u64> {
    chrono::NaiveDateTime::parse_from_str(s, LOG_TS_FORMAT)
        .map(|ndt| ndt.and_utc().timestamp() as u64)
        .map_err(|e| format!("bad timestamp '{s}' (expected UTC '{LOG_TS_FORMAT}', e.g. '2026-08-05 14:32:07'): {e}"))
}

pub fn append_trade_log_line(path: &str, line: &str, header: Option<&str>) {
    let needs_header = header.is_some() && !Path::new(path).exists();
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| -> std::io::Result<()> {
            if needs_header {
                writeln!(f, "{}", header.unwrap())?;
            }
            writeln!(f, "{line}")?;
            Ok(())
        });
    if let Err(e) = result {
        eprintln!("Warning: could not write to trade log ({path}): {e}");
    }
}

pub fn log_row(path: &str, header: Option<&str>, kind: &str,
               pivot_id: Option<Id>, close_id: Option<Id>,
               opened_pivot_id: Option<Id>, prim: &str, proper: &str,
               prim_amount: f64, proper_amount: f64, gain: Option<f64>,
               roi: Option<f64>, apr: Option<f64>, gas_avax: f64, tx_hash: &str,
               snap: &BalanceSnapshot, cum: &CumulativeStats) {
    fn show<T: ToString>(item: Option<T>) -> String {
       item.and_then(|i| Some(i.to_string())).unwrap_or_default()
    }
    fn show6(numba: Option<f64>) -> String {
       numba.and_then(|n| Some(format!("{n:.6}"))).unwrap_or_default()
    }
    fn show8(biggy: Option<f64>) -> String {
       biggy.and_then(|n| Some(format!("{n:+.8}"))).unwrap_or_default()
    }
    fn shows<T: ToString>(v: Vec<Option<T>>) -> String {
       let s: Vec<String> = v.into_iter().map(show).collect();
       s.join("\t")
    }
    let line0 = shows(vec![pivot_id, close_id, opened_pivot_id]);
    let line1 = [kind, &line0, prim, proper].join("\t");
    let line2 = format!("{line1}\t{prim_amount:.8}\t{proper_amount:.8}");
    let line3 = [show8(gain), show6(roi), show6(apr)].join("\t");
    let line4 = format!("{gas_avax:.8}\t{tx_hash}");
    let pen = snapshot_and_cumulative_columns(snap, cum);
    let ult = format!("{}\t{line2}\t{line3}\t{line4}\t{pen}", log_ts());
    
    append_trade_log_line(path, &ult, header);
}

pub fn log_open(path: &str, header: Option<&str>, pivot_id: Id, 
                prim: &str, prim_amount: f64, proper: &str, proper_amount: f64,
                gas_avax: f64, tx_hash: &str, snap: &BalanceSnapshot, 
                cum: &CumulativeStats) {
    log_row(path, header, "OPEN", Some(pivot_id), None, None, prim, proper,
            prim_amount, proper_amount, None, None, None, gas_avax, tx_hash,
            snap, cum);
}

pub fn log_close(path: &str, header: Option<&str>, pivot_id: Id, close_id: Id, 
                 prim: &str, prim_amount: f64, proper: &str, proper_amount: f64,
                 gain: f64, roi: f64, apr: f64, gas_avax: f64, tx_hash: &str, 
                 snap: &BalanceSnapshot, cum: &CumulativeStats) {
    log_row(path, header, "CLOSE", None, Some(close_id), Some(pivot_id), 
            prim, proper, prim_amount, proper_amount, Some(gain),
            Some(roi), Some(apr), gas_avax, tx_hash, snap, cum);
}

pub fn log_misfire(path: &str, header: Option<&str>, prim: &str, proper: &str, prim_amount: f64, proper_amount: f64, tx_hash: &str, snap: &BalanceSnapshot, cum: &CumulativeStats) {
    log_row(path, header, "MISFIRE", None, None, None, prim, proper, prim_amount, proper_amount, None, None, None, 0.0, tx_hash, snap, cum);
}

fn snapshot_and_cumulative_columns(snap: &BalanceSnapshot,
                                   cum: &CumulativeStats) -> String {
    let ans0 = format!("{:.8}\t{:.8}\t{:.8}\t{:.2}\t{:.2}\t{:.2}",
        snap.asset_balance, snap.asset_committed, snap.asset_available,
        snap.undead_balance, snap.undead_committed, snap.undead_available);
    format!("{ans0}\t{:+.8}\t{:+.2}\t{:.8}\t{:.6}\t{:.6}",
        cum.total_gain_asset, cum.total_gain_undead, cum.total_gas_avax,
        cum.avg_roi(), cum.avg_apr())
}

// ----- TESTS -------------------------------------------------------

