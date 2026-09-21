# frignan

Prints the live KyberSwap price for a token, on a given blockchain, quoted 
against USDC.

## Usage

> `frignan` <TOKEN> [--blockchain <BLOCKCHAIN>]

where: 

* `TOKEN` is the ticker symbol, e.g. `BTC`, `BNB`, `AVAX` |
* `BLOCKCHAIN` is the chain name, e.g. `avalanche`, `binance` |

* [source](../../quizzes/src/quiz01/a_frignan/mod.rs)

Flags:

* `-d`, `--debug` | Verbose output |

## Example

`frignan` BTC
answer is $64209.07

## Requirements

* A `<blockchain>.toml` token registry file (e.g. `avalanche.toml`) must exist, 
listing each token's address and decimals.

## Revisions

* 1.2.2, 2026-09-20: use query_quote lib function
* 1.2.1, 2026-09-18: directory-restructuring
* 1.2.0, 2026-09-15: library-refactoring
* 1.1.5, 2026-08-28: CLAP provides default blockchain
* 1.1.4, 2026-08-28: Calling query swap, no longer need wallet address nor 
keystore path
* 1.0.3, 2026-08-18: moved into production
