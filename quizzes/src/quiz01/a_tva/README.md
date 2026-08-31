# `tva`

Bidirectional BTC⇄UNDEAD auto-trader. Every cycle (every hour) checks all open pivots
against a live reverse quote and closes any that show a gain, then commits
remaining capital to new fixed-size pivots in both directions where
available.

## Usage

`$ tva --log-path <path> [--dry-run] [--debug] [--blockchain <chain>] [--btc-trade-amount <amount>] [--undead-trade-amount <amount>]`

`$ tva --log-path <path> [--dry-run] [--debug] [--blockchain <chain>] [--btc-trade-amount <amount>] [--undead-trade-amount <amount>] div [--pct <PERCENT>]`

where:

* `--log-path <path>` required, CLI-only (no env fallback) — path to the
  trade log to replay and append to. A missing or wrong path is a hard
  error: tvá's history is real and pre-existing, so a missing log means
  the path is wrong, not that this is a fresh start.
* `[--dry-run]` checks only — never touches the keystore or sends a tx, no
  log entries written for opens or closes. Applies to `div` too: the cycle
  it runs first moves no funds, and the vault transfer itself becomes a
  preview (amounts computed and printed, `send_tokens` never called).
* `[--debug]` prints the full per-cycle breakdown — wallet status, totals,
  vs starting capital, split preview. Without it, only the actual trade
  events (opened/closed/failed) and one summary line are shown.
* `[--blockchain <chain>]` selects which chain's token registry to load,
  from `data/<chain>.toml` — defaults to `avalanche` (tvá's only chain to
  date), so it can be omitted entirely for existing usage.
* `[--btc-trade-amount <amount>]` amount of BTC to open a new BTC->UNDEAD
  position with each cycle. Defaults to `0.005`.
* `[--undead-trade-amount <amount>]` amount of UNDEAD to open a new
  UNDEAD->BTC position with each cycle. Defaults to `500000`.
* `div [--pct <PERCENT>]` runs the cycle, then sends `--pct` of the
  sendable surplus above starting capital to Vault. Default `25.0`; accepts
  any value `0`–`100`. The remainder ("trim") simply stays in `tva`'s own
  wallet — there's no separate transfer for it.

> n.b: `--log-path`, `--dry-run`, `--debug`, `--blockchain`,
> `--btc-trade-amount`, and `--undead-trade-amount` all belong to the
> top-level command, not to `div` — put them *before* `div` on the command
> line (`tva --log-path tva-trades.log --dry-run --debug div --pct 5`, not
> the other way around).

> n.b: `TVA_WALLET_ADDRESS` and `TVA_KEYSTORE_PATH` must be set as
> environmental variables — distinct names from `arbitrage`'s
> `WALLET_ADDRESS`/`KEYSTORE_PATH`. Both binaries share a keystore-loading
> core, but each has its own wallet, so a shared env var name would let
> whichever was set last in a shell session silently win for both.
> `KEYSTORE_PASSWORD` is optional — set it for unattended runs (e.g. CI);
> omit it locally to be prompted interactively instead. `TVA_EXPECTED_WALLET`
> is optional — set it to hard-fail on a `TVA_WALLET_ADDRESS` mismatch
> instead of silently reading the wrong wallet's balance. `--log-path` has
> no env equivalent — unlike the two above, it must always be passed on
> the command line.

* [source](mod.rs)

## State

All state is derived by replaying `tva-trades.log` from scratch on every
run — there's no separate persisted state file. Log lines start with a raw
Unix timestamp; to read one:

`$ date -d @1785876760`

converts it to your system's local time.

A failed real (non-dry-run) trade attempt writes a `MISFIRE` row instead
of an `OPEN`/`CLOSE` — same replay, but a `MISFIRE` is never mistaken for
a real pivot open or close, it only folds into the running gas/gain
totals. `--dry-run` still writes zero log entries, MISFIRE included.

## Revisions

* 1.7.2, 2026-08-31: slippage adjustment from 5% (500) to 10% (1000).
* 1.7.1, 2026-08-31: adding tests for the new functions added.
* 1.7.0, 2026-08-31: `--btc-trade-amount`/`--undead-trade-amount` replace
the old hardcoded trade-size consts, and `--log-path` is now a required
CLI flag (previously CARGO_MANIFEST_DIR-derived); the cycle summary is now
a scoreboard against starting capital (open pivots, pool ROI/APR, total
gains, gas used, live wallet gas balance).
* 1.4.2, 2026-08-25: removed the keystore_path_var being passed in 
* 1.3.1, 2026-08-24: consistent decimals and slippage adjustment
* 1.3.0 , 2026-08-24: `tva` had some cases were the pivots that can be closed 
were not being closed. That is hopfully addressed now. And, revision on the 
`debug` use.
* 1.2.0, 2026-08-19: real (non-dry-run) trade failures now write a
`MISFIRE` row to `tva-trades.log` via a new `log_misfire` helper in
`trading::auto_trading` (shared with `arbitrage`), instead of console-only
output that vanished once the run ended. Dry-run still writes zero log
entries.
* 1.1.0, 2026-08-19: `tokens.toml` is no longer a compile-time
`include_str!` constant — it's now loaded at runtime from
`data/<blockchain>.toml` via a new `--blockchain` flag (defaults to
`avalanche`, so existing invocations and the `tva.yml` workflow keep
working unchanged).
* 0.24.0, 2026-08-15: after all tests pased - beta-reduction of two duplicate
functions doing the same thing.
* 0.23.0, 2026-08-15: hot-fix to address negative gain and I also corrected
the decimals as the inconsistent precision was casuing weird rounding.
* 0.13.0, 2026-08-07: `--debug` flag added — default output trimmed to
trade events plus one summary line; the full per-cycle breakdown (wallet
status, totals, vs starting capital, split preview) is now opt-in. `div`
now respects the top-level `--dry-run` (previously always a real send,
regardless of the flag). Removed a leftover duplicate startup banner.
* 0.4.0, 2026-08-04: `TVA_WALLET_ADDRESS`/`TVA_KEYSTORE_PATH` — own env var
names, no longer shared with `arbitrage`; sequential pivot id fix (advances
per attempt, not only per success); explicit "No funds moved" on every
pre-transaction failure path
* 0.1.0: initial version — bidirectional BTC⇄UNDEAD pivot trading,
append-only trade log, gas tracking, dry-run mode
