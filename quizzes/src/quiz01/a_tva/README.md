# `tva`

Bidirectional BTC⇄UNDEAD auto-trader. Every cycle (every hour) checks all open pivots
against a live reverse quote and closes any that show a gain, then commits
remaining capital to new fixed-size pivots in both directions where
available.

## Usage

`$ tva [--dry-run] [--debug]`

`$ tva [--dry-run] [--debug] div [--pct <PERCENT>]`

where:

* `[--dry-run]` checks only — never touches the keystore or sends a tx, no
  log entries written for opens or closes. Applies to `div` too: the cycle
  it runs first moves no funds, and the vault transfer itself becomes a
  preview (amounts computed and printed, `send_tokens` never called).
* `[--debug]` prints the full per-cycle breakdown — wallet status, totals,
  vs starting capital, split preview. Without it, only the actual trade
  events (opened/closed/failed) and one summary line are shown.
* `div [--pct <PERCENT>]` runs the cycle, then sends `--pct` of the
  sendable surplus above starting capital to Vault. Default `25.0`; accepts
  any value `0`–`100`. The remainder ("trim") simply stays in `tva`'s own
  wallet — there's no separate transfer for it.

> n.b: `--dry-run` and `--debug` belong to the top-level command, not to
> `div` — put them *before* `div` on the command line
> (`tva --dry-run --debug div --pct 5`, not the other way around).

> n.b: `TVA_WALLET_ADDRESS` and `TVA_KEYSTORE_PATH` must be set as
> environmental variables — distinct names from `arbitrage`'s
> `WALLET_ADDRESS`/`KEYSTORE_PATH`. Both binaries share a keystore-loading
> core, but each has its own wallet, so a shared env var name would let
> whichever was set last in a shell session silently win for both.
> `KEYSTORE_PASSWORD` is optional — set it for unattended runs (e.g. CI);
> omit it locally to be prompted interactively instead. `TVA_EXPECTED_WALLET`
> is optional — set it to hard-fail on a `TVA_WALLET_ADDRESS` mismatch
> instead of silently reading the wrong wallet's balance.

* [source](mod.rs)

## State

All state is derived by replaying `tva-trades.log` from scratch on every
run — there's no separate persisted state file. Log lines start with a raw
Unix timestamp; to read one:

`$ date -d @1785876760`

converts it to your system's local time.

## Revisions

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
