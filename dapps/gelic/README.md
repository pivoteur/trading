# gelic

Old English *gelīc* — "like, similar, alike," the ancestor of Modern English
"like" and the "-ly" suffix. Pronounced **yeh-LEECH**.

A read-only wallet reader. Give it an address and it prints what's actually
in that wallet — no keystore, no trading, no log.

## Usage

```sh
# every token in the wallet
gelic 0x123abc69

# just one token
gelic 0x123abc69 --token BTC

# a different chain (defaults to avalanche)
gelic 0x123abc69 --blockchain binance --token BTC
```

`wallet_address` is a required arg to pass-in, no env fallback — you always say which wallet
to read.

* [source](../../quizzes/src/quiz02/a_gelic/mod.rs) 

## Revisions

* 0.1.0, 2026-09-04: The initial build of `gelic` with some simple tests. 