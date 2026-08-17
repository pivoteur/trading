# `equalizer`

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

`$ equalizer [--dry-run] [--debug] [--slippage-bps <bps>] <TOKEN>`

e.g. `$ equalizer BTC`

where:

* `<TOKEN>` is the ticker of the non-UNDEAD side of the pair -- must already
  have an entry in `tokens.toml`. This is the only thing that changes run to
  run; UNDEAD itself is fixed.
* `[--dry-run]` checks and computes the swap amount only -- never touches
  the keystore or sends a tx. Still writes a log row (outcome `WOULD_SWAP`
  or `SKIPPED_ALREADY_BALANCED`) so a dry-run history is visible too.
* `[--debug]` prints the full quote/approve/swap play-by-play via Doug's
  `book::debug!`/`log!` macros -- without it, a run collapses to one or two
  summary lines.
* `[--slippage-bps]` slippage tolerance in basis points, default `50`
  (0.50%) -- deliberately tighter than `tvá`'s `200`, since hourly runs are
  meant to be closing small gaps, not large ones.

> n.b: `tvá`'s eviornmental variables must be set for this to work.
> _This is subject to change, as `equalizer` may reiceve its own env-vars_
> some reason for this this: 
> just trying to achieve the simplest form of `equalizer` first.

## The math

Each run reads the wallet's UNDEAD balance and `<TOKEN>` balance, then gets
a live KyberSwap quote for what the `<TOKEN>` balance is worth in UNDEAD
right now (the actual executable rate -- no external USD price feed is
used, or needed, since this only ever compares the two against each other).
Solving for `x` (UNDEAD to swap) so both sides land at equal UNDEAD-
denominated value:

```
token_value_in_undead + x == undead_balance - x
x == (undead_balance - token_value_in_undead) / 2
```

If `x <= 0` -- `<TOKEN>` is already at or ahead of parity -- nothing
happens. There is no code path in `mod.rs` that ever calls
`<TOKEN> -> UNDEAD`; "never the other direction" is structural, not a
runtime condition that could be skipped.

* [source](mod.rs)

## Log

Every run appends one line to `equalize-undead-btc.log` (`timestamp / mode /
token / undead_balance / token_balance / token_value_in_undead /
swap_undead / outcome / actual_received / gas_avax / tx_hash`), including
refusals (`SKIPPED_ALREADY_BALANCED`, `NOT_CLEARED`, `FAILED: ...`), not
just successful swaps.

## Revisions

* 0.3.0: clean-ups -- changing most lines that had mismatched types and vars, 
and manual testing to ensure working from a clean state.
* 0.2.0: redundancy deletion -- some blocks derived from ai suggestions 
wasn't what was needed.
* 0.1.0: initial version -- single-token UNDEAD->TOKEN balancer, hourly,
dry-run mode, append-only run log including refusals.
