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

* 1.2.3, 2026-10-22: Handle native token swaps in library code
* 1.2.2, 2026-10-20: show what would be the trade-amounts, even during `DRYRUN`;
`mock_wallet` is now `async`
* 1.2.1, 2026-10-12: moved most of libs to implementation-level
* 1.2.0, 2026-10-08: shift to `Wallet` implementation; make DRY_RUN enum
* 1.1.5, 2026-09-21: externalized token-registry
* 1.1.4, 2026-09-20: reduced trade amounts to f32-size
* 1.1.3, 2026-09-18: directory-restructuring
* 1.1.2, 2026-09-18: parameterize `slippage_bps`
* 1.1.1, 2026-09-17: allow comma-floats as inputs to `floor` and `amount`
* 1.1.0, 2026-09-15: library-refactoring
* 1.0.0, 2026-08-20: released

-----

## Verifications

| blockchain | from   | amt       | to     | received | date       | tx_id   |
| :--------- | :----- | --------: | :----- | -------: | :-----     | :-----: |
| Avalanche  | UNDEAD | 70,000.00 | USDC   | 24.37    | 2026-10-21 | [https://snowtrace.io/tx/0x1d83...1061](https://snowtrace.io/tx/0x1d830b179a37ed61c3816aa5cf57d5bd58888071aa14ff3b1b95952e1774106a) |
| Avalanche  | AVAX   | 0.01      | UNDEAD | 2662.75  | 2026-10-22 | [https://snowtrace.io/tx/0x8486...5582](https://snowtrace.io/tx/0x8486ecd47658980ab94d1532a7af78d0c7722e666f98a6f14e651c4ad1035582) |
| Avalanche  | USDC   | 5.00 | AVAX | 0.54 | 2026-10-22 | [https://snowtrace.io/tx/0xe70fb...82cf](https://snowtrace.io/tx/0xe70fb045d9916f409fe6b8ab9c15f2e7b921be94f1503acf639916b10b8282cf) |
| Binance    | DOGE | 45.00 | USDt   | 3.74    | 2026-10-21 | [https://bscscan.com/tx/0xcd9c...bf1f](https://bscscan.com/tx/0xcd9c1ef96818d2fb023d5c844c892531af8e6b339e13e8c9a790c3663335bf1f)
| Binance  | BNB   | 0.05 | USDt | n/a      | 2026-10-21 | 🚫 |
| Binance | USDt | 1.00 | BNB | 0.0013 | 2026-10-22 | [https://bscscan.com/tx/0x9d49...157a](https://bscscan.com/tx/0x9d49904a074a936a783e8160b8ba716718e1fddef3b38b2de1979c3a788d157a) |
