# Bootstrapping `tvá`'s Trade Log by Hand
### This is if/ when you've built `tva` and you've received the "_Error: Where is tvá's history?_" message
`tvá` refuses to start a brand-new instance with no trade log — a missing log
file almost always means a wrong `--log-path`, not a fresh start, so it hard
errors instead of quietly assuming one. That means the very first two OPEN
rows have to be seeded by hand, once, before `tvá`'s first run. This doc walks
through doing that with nothing but KyberSwap, Snowtrace, and your own
wallet. Auto-opens are handled by a separate program.

## What you're building

`tvá`'s log is a plain **tab-separated** text file (.tsv for MAC users as a .log may give you a completely new error). The first line is a
header naming every column; then, every line after that is one trade event. To
bootstrap a fresh instance you're writing exactly three lines: 
* the header
* then _two_ `OPEN` rows

**Important:** every gap below is a real Tab character, not spaces. If
you're typing this in a plain text editor, press Tab between each field, and
check your editor isn't set to auto-convert tabs to spaces (that setting is
usually called "insert spaces for tabs" — turn it off for this file).

## Step 1 — Open the first two starting pivots on KyberSwap

* Check your wallet as it is now and jot down the amount pf each token 
you are starting with, perhpas in a Google Sheet? 

Using the original `tvá` pivoting instance as an example will be:

1. Swap **0.005 BTC → UNDEAD**.
2. Once that confirms, swap **500,000 UNDEAD → BTC**.

These are `tvá`'s built-in default trade sizes. If this instance was set up
with `--btc-trade-amount` / `--undead-trade-amount` overrides, use those
amounts instead — the wallet needs enough of *both* tokens up front to cover
both starting trades independently (they don't depend on each other and you can set 
however much you'd want to start with).

## Step 2 — Pull each trade's details from Snowtrace

For each of the two transactions you just made:

1. Open `https://snowtrace.io/tx/<the transaction hash>` in a browser.
2. Find the **Timestamp** and switch it to **UTC** (Snowtrace defaults to a
   relative "X mins ago" display — click it to reveal the exact date/time).
   Write it down as `YYYY-MM-DD HH:MM:SS`, 24-hour, UTC. This matters: the
   log is always UTC, never your local timezone.
3. Find **Transaction Fee** — that's the gas cost in $AVAX.
4. Find **Tokens Transferred** (sometimes labeled "ERC-20 Tokens
   Transferred") — that's the exact amount of the destination token that
   landed in your wallet from this swap.
5. Separately, check your wallet's BTC and UNDEAD balances *right after*
   this trade confirmed (your wallet app, or Snowtrace's address page for
   this wallet).

## Column reference

| # | Column | For an OPEN row |
|---|---|---|
| 1 | `timestamp` | UTC time of the tx, from Step 2 |
| 2 | `kind` | always `OPEN` |
| 3 | `pivot_id` | `1` for the first trade, `2` for the second |
| 4 | `close_id` | blank |
| 5 | `opened_pivot_id` | blank |
| 6 | `prim` | the token you swapped *from* (`BTC` then `UNDEAD`) |
| 7 | `proper` | the token you swapped *to* (`UNDEAD` then `BTC`) |
| 8 | `prim_amount` | the fixed amount you fed in, 8 decimals |
| 9 | `proper_amount` | "Tokens Transferred" from Snowtrace, 8 decimals |
| 10 | `gain` | blank |
| 11 | `roi` | blank |
| 12 | `apr` | blank |
| 13 | `gas_avax` | "Transaction Fee" from Snowtrace, 8 decimals |
| 14 | `tx_hash` | the transaction hash |
| 15 | `asset_balance` | your BTC balance right after this trade, 8 decimals |
| 16 | `asset_committed` | see below |
| 17 | `asset_available` | `asset_balance` − `asset_committed` |
| 18 | `undead_balance` | your UNDEAD balance right after this trade, **2 decimals** |
| 19 | `undead_committed` | see below, **2 decimals** |
| 20 | `undead_available` | `undead_balance` − `undead_committed` |
| 21 | `cum_gain_asset` | `+0.00000000` — always zero until a pivot closes |
| 22 | `cum_gain_undead` | `+0.00` — always zero until a pivot closes |
| 23 | `cum_gas_avax` | running total of gas spent so far this log |
| 24 | `avg_roi` | `0.000000` — no closes yet |
| 25 | `avg_apr` | `0.000000` — no closes yet |

Two things that trip people up:

- **Columns 8 and 9 always use 8 decimals, even for UNDEAD.** Only the
  wallet-balance columns (18–20) round UNDEAD to 2 decimals. So the same
  UNDEAD amount can look different in different columns of the same row —
  that's correct, not a mistake.
- **"Committed" isn't an on-chain number** — it's just "how much of this
  token is tied up in a pivot that's still open." For these first two rows:
  - After row 1 (BTC→UNDEAD): the UNDEAD you just received is fully
    committed (nothing else is open yet). BTC committed is still `0`.
  - After row 2 (UNDEAD→BTC): the BTC you just received is fully committed.
    UNDEAD committed carries over unchanged from row 1 — row 2 doesn't touch
    that pivot.

## Step 3 — Worked example

Say the wallet started with 0.05000000 BTC and 5,000,000.00 UNDEAD before
either trade.

**Trade 1: 0.005 $BTC → $UNDEAD**
Snowtrace shows: timestamp `2026-09-01 14:03:22` UTC, fee `0.00187500 $AVAX`,
`497,832.15 $UNDEAD` transferred. Wallet after: `$BTC 0.04500000`, `$UNDEAD 5,497,832.15`.

- `asset_committed` = `0.00000000` (no $BTC pivot yet) → `asset_available` = `0.04500000`
- `undead_committed` = `497832.15` (just received) → `undead_available` = `5497832.15 − 497832.15` = `5000000.00`

**Trade 2: 500,000 $UNDEAD → $BTC**
Snowtrace shows: timestamp `2026-09-01 14:11:47` UTC, fee `0.00192300 $AVAX`,
`0.00499875 $BTC` transferred. Wallet after: `$BTC 0.04999875`, `$UNDEAD 4,997,832.15`.

- `asset_committed` = `0.00499875` (just received) → `asset_available` = `0.04999875 − 0.00499875` = `0.04500000`
- `undead_committed` = `497832.15` (unchanged from row 1) → `undead_available` = `4997832.15 − 497832.15` = `4500000.00`

That gives these three lines (each gap is a Tab):

```
timestamp	kind	pivot_id	close_id	opened_pivot_id	prim	proper	prim_amount	proper_amount	gain	roi	apr	gas_avax	tx_hash	asset_balance	asset_committed	asset_available	undead_balance	undead_committed	undead_available	cum_gain_asset	cum_gain_undead	cum_gas_avax	avg_roi	avg_apr
2026-09-01 14:03:22	OPEN	1				BTC	UNDEAD	0.00500000	497832.15000000				0.00187500	0xAAA1111111111111111111111111111111111111111111111111111111AAA1	0.04500000	0.00000000	0.04500000	5497832.15	497832.15	5000000.00	+0.00000000	+0.00	0.00187500	0.000000	0.000000
2026-09-01 14:11:47	OPEN	2				UNDEAD	BTC	500000.00000000	0.00499875				0.00192300	0xBBB2222222222222222222222222222222222222222222222222222222BBB2	0.04999875	0.00499875	0.04500000	4997832.15	497832.15	4500000.00	+0.00000000	+0.00	0.00379800	0.000000	0.000000
```

(Your real numbers will differ — this is just to show the exact shape.)

## Step 4 — Save it

Save those three lines, in that order, as the file at this instance's
`--log-path`. That's it — tvá reads this exactly the same way whether a
program wrote it or a person did, so its very next run picks up right where
these two pivots leave off.

## Copying into Google Sheets, If You Want

Once the file has real tab characters, open it in any plain text editor,
select all three lines, and hard-paste (ctrl + shift + v) directly into a 
Google Sheets tab — Sheets recognizes tab-separated clipboard text and 
splits it into columns automatically, header row included.
