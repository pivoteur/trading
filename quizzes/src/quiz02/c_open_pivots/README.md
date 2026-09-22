# `c_open_pivots`

* We know HOWTO [check open pivots](../a_opens)
* We know HOTWO [check pivot asset balances on a wallet](../b_balances)

Let's now combine the above, then, with the assets available, open new pivots
(constrainted by some maximum amount).

* We'll use [ceap](../../../../dapps/ceap) to execute the trades to open the
pivots
* We also need to record the new opened pivots in the open pivot table.

Of course, eventually, we'll need to close the opened pivots, but that's the 
next dapp. We need opened pivots in order to close pivots, so let's get'r done!

