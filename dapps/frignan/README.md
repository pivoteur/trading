# frignan

Prints the live KyberSwap price for a token, on a given blockchain, quoted against USDC.

## Usage

`frignan` <FROM_TOKEN> <AMOUNT> <BLOCKCHAIN>

where: 

* `FROM_TOKEN` is the ticker symbol, e.g. `BTC`, `BNB`, `AVAX` |
* `AMOUNT` is the amount of `FROM_TOKEN` to quote |
* `BLOCKCHAIN` is the chain name, e.g. `avalanche`, `binance` |

* [source](../../quizzes/src/quiz02/b_frignan/mod.rs)

Flags:

* `-d`, `--debug` | Verbose output |

## Example

`frignan` <BTC> <1> <avalanche>
answer is Ok(DryRunWouldClear { quoted_amount_out: 64209.07... })

## Requirements

* `WALLET_ADDRESS` and `TVA_KEYSTORE_PATH` env vars must be set.
* A `<blockchain>.toml` token registry file (e.g. `avalanche.toml`, `binance.toml`) must exist, listing each token's address and decimals.

## Revisions

* 1.1.5, 2026-08-28: CLAP provides default blockchain
* 1.1.4, 2026-08-28: Calling query swap, no longer need wallet address nor 
keystore path
* 1.0.3, 2026-08-18: moved into production
