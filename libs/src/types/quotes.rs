use std::{ collections::HashMap, sync::LazyLock };

use chrono::{ NaiveDateTime, NaiveDate, Utc };
use serde::{ Serialize, Deserialize };
use tokio::sync::RwLock;

use libs::types::{ blockchains::Blockchain, quotes::Quotes };
use super::tokens::TokenRegistry;
use book::{ debug, err_utils::ErrStr };

const FLUSH: i64 = 5 * 60;  // 5 minutes
const QUOTES_DIR: &'static str = "data/quotes";

#[derive(Clone,Serialize,Deserialize)]
pub struct ActiveQuotes {
   last_updated: NaiveDateTime,
   quotes: Quotes
}

// 1. Facade: What your Wallets call in production.
//    Automatically wires up `TokenRegistry::fetch_quotes` as the fetcher.
pub async fn fetch_active_quotes(registry: &TokenRegistry,
                                 chain: &Blockchain, auth: &str,
                                 debug: bool) -> ErrStr<Quotes> {
   get_active_quotes(chain,
            async |dt| { registry.fetch_quotes(auth, dt).await },
            debug).await
}

// Production function uses the global memory cache + "data/" dir
async fn get_active_quotes<F>(chain: &Blockchain, fetcher: F, debug: bool)
      -> ErrStr<Quotes>
  where F: AsyncFnOnce(&NaiveDate) -> ErrStr<Quotes> {
   get_or_fetch_quotes(&QUOTE_CACHE, chain, FLUSH, Path::new(QUOTES_DIR),
                       fetcher, debug).await
}

// ----- IMPLEMENTATION CODE --------------------------------------------------

type QuoteMap = RwLock<HashMap<Blockchain, ActiveQuotes>>;

static QUOTE_CACHE: LazyLock<QuoteMap> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

use std::fs;
use std::path::{ Path, PathBuf };

fn cache_path(data_dir: &Path, chain: &Blockchain) -> PathBuf {
    data_dir.join(format!("{}.json", chain.blockchain()))
}

fn load_from_disk(data_dir: &Path, chain: &Blockchain, debug: bool)
      -> Option<ActiveQuotes> {
   debug!("load_from_disk", debug);
   log!("fetching quotes from store");
    let contents = fs::read_to_string(cache_path(data_dir, chain)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn save_to_disk(data_dir: &Path, chain: &Blockchain, active: &ActiveQuotes,
                debug: bool) {
   debug!("save_to_disk", debug);
    let _ = fs::create_dir_all(data_dir); // Ensure data/ exists
    if let Ok(json) = serde_json::to_string_pretty(active) {
      log!("writing quotes to store");
        let _ = fs::write(cache_path(data_dir, chain), json);
    } else {
       log!("could not write quotes to store; JSON conversion failed");
   }
}

// 3. Testable core: Isolated cache map, configurable flush duration, 
//    temp dir, and mockable fetcher.
async fn get_or_fetch_quotes<F>(
    cache_lock: &QuoteMap,
    chain: &Blockchain,
    flush_seconds: i64,
    data_dir: &Path,
    fetcher: F, debug: bool) -> ErrStr<Quotes>
   where
      F: AsyncFnOnce(&NaiveDate) -> ErrStr<Quotes> {
   debug!("get_or_fetch_quotes", debug);

    let now = Utc::now().naive_utc();

    // 1. Memory check
    {
      log!("memory check");
        let cache = cache_lock.read().await;
        if let Some(entry) = cache.get(chain) {
            if (now - entry.last_updated).num_seconds() <= flush_seconds {
                return Ok(entry.quotes.clone());
            }
        }
    }

    let mut cache = cache_lock.write().await;

    // Double-check memory

    log!("double-mem-check");

    if let Some(entry) = cache.get(chain) {
        if (now - entry.last_updated).num_seconds() <= flush_seconds {
            return Ok(entry.quotes.clone());
        }
    }

    // 2. Disk check

    log!("disk-check");

    if let Some(disk_entry) = load_from_disk(data_dir, chain, debug) {
        if (now - disk_entry.last_updated).num_seconds() <= flush_seconds {
            let quotes = disk_entry.quotes.clone();
            cache.insert(chain.clone(), disk_entry);
            return Ok(quotes);
        }
    }

    // 3. API check

    log!("coin-gecko fetch");
    let now = Utc::now().naive_utc();
    let fresh_quotes = fetcher(&now.date()).await?;
    let entr = ActiveQuotes { last_updated: now, quotes: fresh_quotes.clone() };

    save_to_disk(data_dir, &chain, &entr, debug);
    cache.insert(chain.clone(), entr);

    Ok(fresh_quotes)
}

// ----- TESTS -------------------------------------------------------

#[cfg(not(tarpaulin_include))]
#[cfg(test)]
mod functional_tests {
   use super::*;
   use paste::paste;
   use book::{ create_testing, csv_utils::list_csv, utils::{ get_env, now } };
   use libs::types::blockchains::Blockchain::*;
   use crate::implementation::fetchers::tokens::fetch_token_registry;

   create_testing!("types::quotes");

   async fn fetch_active_quotes_for(chain: &Blockchain) -> ErrStr<()> {
      let registry = fetch_token_registry(chain).await?;
      let auth = get_env("COIN_GECKO_API_KEY")?;
      let quotes = fetch_active_quotes(&registry, chain, &auth, true).await?;
      println!("{chain} active quotes:\n\n{}", list_csv(&[quotes], true));
      Ok(())
   }

   run!("fetch_active_quotes_avalanche",
        now(fetch_active_quotes_for(&AVALANCHE))?);
   run!("fetch_active_quotes_binance", now(fetch_active_quotes_for(&BINANCE))?);
}

#[cfg(not(tarpaulin_include))]
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use std::sync::{ atomic::{AtomicUsize, Ordering}, Arc };
    use tempfile::tempdir;
    use libs::types::quotes::sample_data::sample_quotes_maker;

    // --- Test Helpers (adjust to match your libs::types constructors) ---

    fn sample_chain() -> Blockchain { Blockchain::AVALANCHE }

    fn make_quotes(price: f32) -> Quotes {
        sample_quotes_maker(&[("ETH", price)])
    }

    // --- Unit Tests -----------------------------------------------------

    #[tokio::test]
    async fn cache_miss_fetches_and_persists_to_memory_and_disk() {
        let cache = RwLock::new(HashMap::new());
        let dir = tempdir().unwrap();
        let chain = sample_chain();
        let calls = Arc::new(AtomicUsize::new(0));

        let calls_clone = calls.clone();
        let expected = make_quotes(100.0);

        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                Ok(make_quotes(100.0))
            },
            false,
        )
        .await
        .unwrap();

        assert_eq!(res, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Verify both memory cache and disk file were populated
        assert!(cache.read().await.contains_key(&chain));
        assert!(cache_path(dir.path(), &chain).exists());
    }

    #[tokio::test]
    async fn memory_hit_skips_disk_and_fetcher() {
        let cache = RwLock::new(HashMap::new());
        let dir = tempdir().unwrap();
        let chain = sample_chain();
        let expected = make_quotes(200.0);

        // Seed memory directly
        cache.write().await.insert(
            chain.clone(),
            ActiveQuotes {
                last_updated: Utc::now().naive_utc(),
                quotes: expected.clone(),
            },
        );

        // Delete/ensure no disk file exists; fetcher panics if called
        assert!(!cache_path(dir.path(), &chain).exists());

        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| {
                  panic!("Fetcher should not be called on a fresh memory hit!")
            },
            false,
        )
        .await
        .unwrap();

        assert_eq!(res, expected);
    }

    #[tokio::test]
    async fn disk_hit_promotes_to_memory_and_skips_fetcher() {
        let cache = RwLock::new(HashMap::new()); // Cold memory
        let dir = tempdir().unwrap();
        let chain = sample_chain();
        let expected = make_quotes(300.0);

        // Pre-populate valid JSON on disk
        let disk_entry = ActiveQuotes {
            last_updated: Utc::now().naive_utc(),
            quotes: expected.clone(),
        };
        save_to_disk(dir.path(), &chain, &disk_entry, false);

        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| {
               panic!("Fetcher should not be called on a fresh disk hit!")
            },
            false,
        )
        .await
        .unwrap();

        assert_eq!(res, expected);
        // Verify entry was promoted into the in-memory HashMap
        assert!(cache.read().await.contains_key(&chain));
    }

    #[tokio::test]
    async fn expired_memory_and_disk_triggers_refresh() {
        let cache = RwLock::new(HashMap::new());
        let dir = tempdir().unwrap();
        let chain = sample_chain();

        let stale = ActiveQuotes {
            last_updated: Utc::now().naive_utc() - Duration::seconds(400),
            quotes: make_quotes(100.0),
        };

        // Seed both memory and disk with expired entries (> 300s old)
        cache.write().await.insert(chain.clone(), stale.clone());
        save_to_disk(dir.path(), &chain, &stale, false);

        let updated = make_quotes(550.0);
        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| { Ok(make_quotes(550.0)) },
            false
        )
        .await
        .unwrap();

        assert_eq!(res, updated);
        // Verify disk was overwritten with the refreshed quotes
        let reloaded = load_from_disk(dir.path(), &chain, false).unwrap();
        assert_eq!(reloaded.quotes, updated);
    }

    #[tokio::test]
    async fn corrupted_disk_file_falls_back_to_fetcher() {
        let cache = RwLock::new(HashMap::new());
        let dir = tempdir().unwrap();
        let chain = sample_chain();

        // Write invalid JSON to the cache path
        fs::write(cache_path(dir.path(), &chain), "{ broken json !!!").unwrap();

        let fresh = make_quotes(420.0);
        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| { Ok(make_quotes(420.0)) },
            false
        )
        .await
        .unwrap();

        assert_eq!(res, fresh);
        // Verify valid JSON replaced the corrupted file
        assert!(load_from_disk(dir.path(), &chain, false).is_some());
    }

    #[tokio::test]
    async fn fetcher_error_propagates_and_preserves_existing_cache() {
        let cache = RwLock::new(HashMap::new());
        let dir = tempdir().unwrap();
        let chain = sample_chain();

        let stale_quotes = make_quotes(100.0);
        let stale = ActiveQuotes {
            last_updated: Utc::now().naive_utc() - Duration::seconds(400),
            quotes: stale_quotes.clone(),
        };
        cache.write().await.insert(chain.clone(), stale);

        let res = get_or_fetch_quotes(
            &cache,
            &chain,
            300,
            dir.path(),
            async |_dt| { Err("CoinGecko 429 Too Many Requests".into()) },
            false
        )
        .await;

        assert!(res.is_err());
        // Ensure stale memory entry wasn't wiped out when API failed
        assert_eq!(
            cache.read().await.get(&chain).unwrap().quotes,
            stale_quotes
        );
    }
}

