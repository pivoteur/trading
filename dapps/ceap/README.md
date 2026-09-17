# ceap

Trades one token for another on a given blockchain, via KyberSwap.

## Usage

cargo run -- <BLOCKCHAIN> <FROM_TOKEN> <AMOUNT> <TO_TOKEN> [OPTIONS]

* [source](../../quizzes/src/quiz02/c_ceap/mod.rs)

| Arg | Description |
|---|---|
| `BLOCKCHAIN` | Chain name, e.g. `avalanche`, `binance` |
| `FROM_TOKEN` | Ticker to sell, e.g. `BTC` |
| `AMOUNT` | Amount of `FROM_TOKEN` to trade |
| `TO_TOKEN` | Ticker to buy, e.g. `ETH` |

Flags:

| Flag | Description |
|---|---|
| `--live` | Actually execute the trade. Without it, ceap always dry-runs. |
| `--floor <AMOUNT>` | Minimum acceptable output. Required with `--live`. |
| `--dry-run` | Force a dry run even if `--live` is also passed. |
| `-d`, `--debug` | Verbose output. |

## Examples

$ cargo run avalanche BTC 1 ETH
$ cargo run binance BTC 1 USDT --live --floor 63000

## Requirements

- `WALLET_ADDRESS` and `TVA_KEYSTORE_PATH` env vars must be set.
- A `<blockchain>.toml` token registry file must exist alongside the binary, listing each token's address and decimals.

Dry-run by default — nothing moves unless `--live` and `--floor` are both set.

-----

## Revision history

* 1.1.1, 2026-09-17: allow comma-floats as inputs to floor and amount
* 1.1.0, 2026-09-15: library-refactoring
* 1.0.0, 2026-08-20: released
