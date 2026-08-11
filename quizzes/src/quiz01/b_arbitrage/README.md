# `arbitrage`

Manages any number of `TOKEN <-> UNDEAD` pivot pools, plus a generic
`calls.csv`-driven mode for manual and batch trades on the side.

## The two halves of this program

**The UNDEAD pivot system** (`new`, and the default survey run with no
subcommand) grows both sides of a pool symmetrically, same principle as
`tvá`: open a pivot, and later close it only if closing actually returns
more than was originally spent — never at a loss. Each pool gets its own
log, so pools never share state or interfere with each other.

**`trade`/`calls`** are separate, generic tools that operate directly on
`calls.csv` rows — not necessarily UNDEAD-related, though in practice most
rows will be. They keep their own flat log, distinct from the per-pool logs.

## Usage

`$ arbitrage [--dry-run] [--debug]`

No subcommand = the default survey: walks every pool that has an existing
log (found by scanning `data/` for `*_undead-trades.log` files — not a
hardcoded list, so a pool bootstrapped via `new` is picked up automatically
next time), and closes whatever's ready to close in each. **Opening new
positions during the survey is not wired up to real amounts yet** — see
the note on `open_trade_amount` in `mod.rs`. Sizing capital automatically
is a decision that needs an explicit call per pool, not something to guess
at from wallet balance, so until that's filled in, the survey only closes.

`$ arbitrage [--dry-run] [--debug] new <token> <amount> [--slippage-bps <bps>]`

Bootstraps a brand-new pool: opens `<token> -> UNDEAD` for `<amount>`,
then `UNDEAD -> <token>` using exactly the UNDEAD the first leg actually
returned (not a fresh quote — that's already the live rate for that exact
amount). Both land as separate OPEN pivots in the new pool's log at
`data/<token>_undead-trades.log`. Refuses to run if that pool already has
a log — `new` is for bootstrapping only, once.

* `<token>`  must already be in `tokens.toml` — nothing is ever looked up
  or inferred live; add the entry there first
* `<amount>` is how much of `<token>` to open the first leg with

`$ arbitrage [--dry-run] [--debug] trade <ix> <amount> <min_floor> --root-url <url> [--slippage-bps <bps>]`

Closes (or opens — direction is whatever the row itself says) exactly one
row from `calls.csv`, by its `ix`. Direction is fixed entirely by the
row's own `pivot_token -> proposed_token`; `<amount>`/`<min_floor>` are
yours to set, independent of whatever `calls.csv` suggests for that row.

`$ arbitrage [--dry-run] [--debug] calls --root-url <url> [--slippage-bps <bps>]`

Reads `calls.csv` and executes **every** row or **none** — true go/no-go.
Every row is checked against its own `gain_10_percent` floor first; only
if all of them clear does any trade execute. One honest limit: each row
is still its own on-chain transaction, not one atomic batch, so this
validates all-or-nothing but can't *execute* as a single atomic unit — a
later row's price could still move between validation and its own turn.
Each trade still re-checks its own floor immediately before it fires, so
nothing ever executes below its floor regardless.

where, across all subcommands:

* `[--dry-run]`      checks only — never touches the keystore or sends a tx
* `[--debug]`        prints the full approve/quote/swap play-by-play,
  including the raw KyberSwap response — without it, a trade collapses to
  one line ("Trading in progress...")
* `[--slippage-bps]` is the slippage tolerance in basis points, e.g. `50` (default)
* `[--root-url]`     is where `calls.csv` is fetched from — falls back to
  the `PIVOT_URL` env var if omitted, e.g. `https://raw.githubusercontent.com/pivoteur/pivoteur.github.io`

> n.b: `--dry-run` and `--debug` belong to the top-level command, not the
> subcommand — put them *before* the subcommand name
> (`arbitrage --dry-run --debug new PAXG 2.0`, not the other way around).

> n.b: `WALLET_ADDRESS` and `KEYSTORE_PATH` must be set as environmental
> variables — arbitrage's own names, distinct from `tva`'s
> `TVA_WALLET_ADDRESS`/`TVA_KEYSTORE_PATH`. Both binaries share a
> keystore-loading core (`libs::auto_trading`), but each has its own
> wallet, so the env var names can't be shared without one silently
> overwriting the other in the same shell session. `KEYSTORE_PASSWORD` is
> optional — set it for unattended runs (e.g. CI); omit it locally to be
> prompted interactively instead.

## Per-pool logs

Each pool's log lives at `data/<token>_undead-trades.log`, a 24-column
TSV — same shape and columns as `tvá`'s log (with `btc_*` generalized to
`token_*`, since it's whichever non-UNDEAD token that pool trades). The
header row is written automatically the first time a pool's log is
created, so it's always there to copy straight into Google Sheets.

* [source](mod.rs)

## Revisions

* 0.12.0, 2026-08-07: major rework — `arbitrage` now manages any number
of `TOKEN <-> UNDEAD` pivot pools (`new` to bootstrap one, no subcommand
to survey and close across all of them), each with its own per-pool TSV
log matching `tvá`'s format; `trade` now closes/opens a specific
`calls.csv` row by `ix` with direction fixed by the row, rather than a
fixed BTC/ETH pair (`Direction`/`--flip` removed); `calls` is now a true
all-or-nothing batch — every row validated against its `gain_10_percent`
floor before anything executes, one failing row means none execute.
Opening new positions in the default survey is intentionally not wired
to real amounts yet (see `open_trade_amount` in `mod.rs`) — sizing that
is a deliberate follow-up, not an oversight.
* 0.11.0: `--debug` flag added — a trade defaults to one
concise line instead of the full approve/quote/swap narration (this is
the same collapse `tva` got; the underlying play-by-play lives in the
shared `libs::auto_trading`, so both binaries drive it off their own
`--debug` now instead of it always printing). `--dry-run` moved from a
per-subcommand flag to top-level, alongside `--debug` — same ordering
rule applies to both (before `trade`/`calls`, not after)
* 0.10.1, 2026-08-04: `wallet_address_from_env`/`load_signer` now take the
env var name as a parameter rather than a hardcoded one — no behavior
change here (arbitrage still reads `WALLET_ADDRESS`/`KEYSTORE_PATH`), but
this is what makes distinct `tva`-specific names possible without
duplicating the shared signing core; every pre-transaction failure in
`load_signer` now says "No funds moved" explicitly
* 0.10.0: moved to `quizzes/src/quiz12/arbitrage/` (from `quiz11`); shared
live-fund-moving core (keystore load/verify, EIP-1559 fee buffering, exact-
amount approval, KyberSwap quoting, swap execution) extracted into
`libs/src/auto_trading.rs`, now used by both `arbitrage` and the new `tva`
binary — `mod.rs` reduced from ~786 to ~356 lines by delegating to it; gas
tracking added (`approve_exact_amount`/`send_swap_tx`/`execute_trade` now
return actual AVAX cost from real transaction receipts)
* 0.9.0, 2026-07-22: added `calls` subcommand — reads `calls.csv`, executes
any row the wallet can fully cover, refuses the rest; split into `trade`/ `calls` subcommands
* 0.8.0: simplified `--direction <normal|flipped>` down to a plain `--flip` flag
* 0.7.0: renamed `Direction` variants to `Normal`/`Flipped` to match the
`reinvested`/`distributed` `flipped` convention
* 0.5.0: bidirectional trading (`BTC -> ETH` and `ETH -> BTC`)
* 0.4.0: production-readiness pass — re-quote after keystore unlock,
EIP-1559 fee/gas buffering, dry-run mode, `f64` precision, HTTP timeouts, persistent trade log
* 0.3.0: initial working version — single direction, interactive keystore password only
