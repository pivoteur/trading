# `maegen`

Keeps a wallet's UNDEAD and one other token at as close to a 50/50 USD-value
split as possible, by swapping UNDEAD -> `<token>` -- **never** the other
direction. Runs hourly (see the workflow) rather than once a day so that a
wallet that grows quickly between runs never needs a single large,
high-slippage catch-up swap; the gap being closed each hour should stay
small.

Unlike `tvá`/ `arbitrage`, this program holds no position history -- it isn't
opening/closing pivots to realize a gain, it's just checking "what's the
split right now?" and correcting it. Every run is independent; there's
nothing to replay.

## Usage

`$ maegen [--dry-run] [--debug] [--slippage-bps <bps>] [--wallet-address <addr>] [--keystore-path <path>] <BLOCKCHAIN> <TOKEN>`

e.g. `$ maegen avalanche BTC`

where:

* `<BLOCKCHAIN>` selects which chain's token registry to load, from
  `data/<BLOCKCHAIN>.toml` -- e.g. `avalanche` reads `data/avalanche.toml`.
* `<TOKEN>` is the ticker of the non-UNDEAD side of the pair -- must already
  have an entry in that file. This is the only thing that changes run to
  run within a given chain; UNDEAD itself is fixed.
* `[--wallet-address]` / `[--keystore-path]` default to `TVA_WALLET_ADDRESS`
  / `TVA_KEYSTORE_PATH` (tvá's own wallet), but can point at any other
  wallet, as long as its tokens are in that chain's `data/<BLOCKCHAIN>.toml`.
* `[--dry-run]` checks and computes the swap amount only -- never touches
  the keystore or sends a tx. Still writes a log row (outcome `WOULD_SWAP`)
  when a swap would have happened; runs that skip because there's nothing
  to do are not logged, dry-run or not.
* `[--debug]` prints extra play-by-play via `book::debug!` -- without it, a
  run collapses to one or two summary lines.
* `[--slippage-bps]` slippage tolerance in basis points, default `50`
  (0.50%) -- deliberately tighter than `tvá`'s `200`, since hourly runs are
  meant to be closing small gaps, not large ones.

> n.b: the scheduled `maegen.yml` workflow always invokes this as
> `maegen avalanche BTC`, and always passes `--wallet-address` /
> `--keystore-path` explicitly, sourced from the `VAULT_ADDRESS` /
> `VAULT_KEYSTORE_PATH` secrets -- **not** the `TVA_WALLET_ADDRESS` /
> `TVA_KEYSTORE_PATH` defaults above. In production this never touches
> tvá's wallet; those defaults only apply to a manual, local invocation
> that omits `--wallet-address`/`--keystore-path` entirely.

* [source](../../quizzes/src/quiz01/c_maegen/mod.rs)

## The math

Each run reads the wallet's UNDEAD balance and `<TOKEN>` balance, then gets
one live KyberSwap quote -- half the UNDEAD balance, swapped to `<TOKEN>` --
and uses that rate to work out what the wallet's existing `<TOKEN>` balance
is worth in UNDEAD right now (the actual executable rate -- no external USD
price feed is used, or needed, since this only ever compares the two
against each other). Solving for `x` (UNDEAD to swap) so both sides land at
equal UNDEAD-denominated value:

## Revisions

* 1.1.0, 2026-08-24: not only in prod how corrected the keystore_path 
misreading on real runs. Also defaulted to the `VAULT_ADDRESS`, not `tva`'s
