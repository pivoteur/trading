# d_gemynd

A read-only status reporter for the auto-traders in this repo. Point it at
one trader by name and it prints a snapshot of that trader's wallet and
(if it keeps one) its trade log — current balances, open pivot count,
opens/closes, realized gains, gas spent, and average ROI/APR.

`gemynd` never touches a keystore and never signs or sends a transaction.
It only reads: live balance queries (the same `wallet_balance` call every
other trader here uses) and, optionally, a trader's existing trade log
via `replay_log`.

## Usage

`gemynd` <name>

Where:

`<name>` is the trader's short name, e.g.:

`gemynd` <tva>

Flags:

- `--blockchain <chain>` — which `data/{chain}.toml` token registry to load. Defaults to `avalanche`.
- `-d`, `--debug` — print the wallet address and trade-log path it resolved, before the report.

## Configuration

`meta` looks up two environment variables per trader, built from `<name>`
uppercased:

- `{NAME}_WALLET_ADDRESS` — **required**. The trader's public wallet address.
- `{NAME}_TRADE_LOG` — **optional**. A filename inside `data/` (e.g. `tva-trades.log`). If set, `meta` replays that log for pivot/gain stats. If unset, it falls back to a plain balance-only report — the shape a trader like `maegen`, which doesn't keep a pivot log, gets automatically.

Example, for `tva`:
TVA_WALLET_ADDRESS=0x... TVA_TRADE_LOG=tva-trades.log meta tva

## Example output

With a trade log set:
=== tva ===
wallet 0x1234...
BTC balance 0.0512 (committed 0.0100, available 0.0412)
UNDEAD balance 5123456.12345678 (committed 500000.00000000, available 4623456.12345678)
open pivots 7
opens / closes 67 / 47
realized gain BTC +0.0001 UNDEAD +538506.29404137
gas spent 0.31994 AVAX
avg roi / apr 2.32% / 428.83%

Without one (balance-only):
=== vault ===
wallet 0x5678...
BTC balance 0.0512
UNDEAD balance 5123456.12345678

## Revisions

* 0.1.0, 2026-08-25: New rust program that will give you the helth of
any auto-trader. Just pass-in the name of the program you want a health-
check on.
