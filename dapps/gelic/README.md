# gelic

Old English *gelīc* — "like, similar, alike," the ancestor of Modern English
"like" and the "-ly" suffix. Pronounced **yeh-LEECH**.

A read-only wallet reader. Give it an address and it prints what's actually
in that wallet — no keystore, no trading, no log.

## Usage

```sh
# every token in the wallet
gelic 0x123abc69

# a different chain (defaults to avalanche)
gelic 0x123abc69 --blockchain binance
```

`wallet_address` is a required arg

* [source](../../quizzes/src/quiz01/b_gelic/mod.rs) 

## Revisions

* 1.1.4, 2026-09-20: returns USDC price as $1.00 (instead of erroring out)
* 1.1.2 and 1.1.3, 2026-09-20: using fetch_wallet_balances from trading-libs
* 1.1.1, 2026-09-18: directory-restructuring
* 1.1.0, 2026-09-15: library-refactoring
* 0.1.0, 2026-09-04: The initial build of `gelic` with some simple tests. 
