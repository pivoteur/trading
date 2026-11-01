use std::fmt;
use strum::EnumString;

#[derive(Debug, Clone, PartialEq, PartialOrd, Ord, Eq, Hash, EnumString)]
#[strum(serialize_all = "UPPERCASE")]
pub enum Execution { DRYRUN, LIVE }

impl fmt::Display for Execution {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(f, "{:?}", self)
   }
}

use Execution::*;

impl From<bool> for Execution {
   fn from(b: bool) -> Self {
      if b { DRYRUN } else { LIVE }
   }
}
