# `a_opens`

## Quiz 02a: compute assets committed to open pivots

Problem:

We want to open new pivots. To do that, we need to know how much of the asset
the wallet has (which [ `gelic` ](../../quiz01/b_gelic) nicely covers), but we
also need to know how much of the asset is already committed to 
previously-opened pivots.

Write a dapp that scans the open pivots of an open pivot table (a [sample
BTC+ETH open pivot table here](../../../data/pivots/open/raw/btc-eth.tsv))
and returns the amount of the committed assets.

Use whatever is available to you, including the Pivot [protocol repository
libraries](https://github.com/pivoteur/protocol/tree/main/libs).

[solution](mod.rs)
