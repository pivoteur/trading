# `sendan`

`sendan` (Old English: "to send") is a one-shot ERC-20 transfer -- sends a
given amount of a token from a keystore-controlled wallet to any address you
name. Unlike `tvá`/`maegen`, it holds no state and makes no trading
decisions; it's a single command that either sends or doesn't. Every
invocation is independent -- there's nothing to replay.

## Usage

`$ sendan [--dry-run] [--debug] [--wallet-address <addr>] [--keystore-path <path>] <BLOCKCHAIN> <AMOUNT> <TOKEN> <TO_ADDRESS>`

e.g. `$ sendan avalanche 1100 UNDEAD 0x12345...`

where:

* `<BLOCKCHAIN>` selects which chain's token registry to load, from
  `data/<BLOCKCHAIN>.toml` -- e.g. `avalanche` reads `data/avalanche.toml`.
* `<AMOUNT>` how much to send, e.g. `1100`.
* `<TOKEN>` the ticker to send, e.g. `UNDEAD` -- must have an entry with a
  real contract address in `data/<BLOCKCHAIN>.toml`. Native coins (no
  contract address) aren't supported -- `sendan` only does ERC-20
  `transfer()` calls.
* `<TO_ADDRESS>` destination wallet address. Checked for shape (`0x` +
  40 hex characters) before anything touches the keystore or the network --
  a malformed address errors out immediately instead of being baked into
  the transfer calldata.
* `[--wallet-address]` / `[--keystore-path]` default to `TVA_WALLET_ADDRESS`
  / `TVA_KEYSTORE_PATH` (tvá's own wallet), but can point at any other
  wallet, as long as its tokens are in that chain's `data/<BLOCKCHAIN>.toml`.
* `[--dry-run]` validates the address and checks the wallet's balance only
  -- never touches the keystore or sends a tx. Still writes a log row
  (outcome `WOULD_SEND`) when the send would have gone through; a bad
  address or insufficient balance is never logged, dry-run or not.
* `[--debug]` prints extra play-by-play via `book::debug!` -- without it,
  a run collapses to one or two summary lines.

> n.b: `<BLOCKCHAIN>` only selects which `data/<BLOCKCHAIN>.toml` is used to
> look up the token's contract address/decimals and to check the wallet's
> balance -- the transfer itself always executes on Avalanche C-Chain, same
> as `tvá`. There's currently nowhere else it could execute; this only
> matters if `sendan` is ever pointed at a chain other than `avalanche`.

> n.b: `SENDAN_KEYSTORE_PATH` you'll see in the source is an internal
> implementation detail, not something to set yourself -- `sendan` sets it
> automatically from whatever `--keystore-path` resolves to, right before
> signing. `TVA_WALLET_ADDRESS`/`TVA_KEYSTORE_PATH` are the actual defaults
> if you omit `--wallet-address`/`--keystore-path`; `KEYSTORE_PASSWORD` is
> optional, same as `tvá` -- set it for unattended runs, omit it locally to
> be prompted interactively instead.

* [source](../../quizzes/src/quiz01/d_sendan/mod.rs)

## State

No pivots, no persisted position -- each run either sends or errors, and
that's the whole story. Every send attempt (real or dry-run) still writes a
row to `sendan-sends.log` for an audit trail. Log lines start with a raw
Unix timestamp; to read one:

`$ date -d @1785876760`

converts it to your system's local time.

A real (non-dry-run) send failure -- a reverted or dropped tx, a signer
error -- writes an outcome of `FAILED: <reason>` rather than silently
printing to a console that nobody's watching. A bad address or an
insufficient balance is caught before anything is attempted and isn't
logged at all, dry-run or not -- there was never a real send to record.

## Revisions

* 0.1.0, 2026-08-19: initial version -- one-shot ERC-20 transfer to any
  address, address-shape validation, balance pre-check, append-only send
  log with `WOULD_SEND`/`SENT`/`FAILED` outcomes.
