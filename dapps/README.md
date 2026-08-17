# dapps

Contains the decentralized applications to automate trades.

## `tvá`
* [source]("../../../quizzes/src/quiz01/a_tva/mod.rs")
* An auto-trader that pivots `$BTC` and `$UNDEAD` every hour.

## `arbitrage`
* [source]("../../../quizzes/src/quiz01/b_arbitrage/mod.rs")
* A swiss army knife of an auto-trader;
- `arbitrage` running the command by itself will run a full surgvey on every opened
pivot to see if any of the pivots can be closed and logs its results. 
Hard-errors if you cannot.
- `arbitrage calls` allows the user to trade 100% of what calls.csv
is recommending. This sub-command is a _go-or-no-go_ for each row listed. If the 
capital isn't availbe in your wallet, `arbitrage calls` will skip said row
and try to close 100% of the next row. Hard-errors if you cannot.
- `arbitrage trade` allows the user to trade one row at a time from calls.csv.
This sub-command will need the row number you want, the new amount to pivot and a 
minimum floor. e.g. `arbitrage` `trade` <2> <2.3398> <8140000> 
Hard-errors if you cannot.
- `arbitrage new` allows the use to open two new pivots with any token <-> $UNDEAD
then, auto-logs to a desired sub-dir. Hard-errors if you cannot.
- `arbitrage tva` allows the user to open two new pivots with any token to any token 
and logs for the user too.

## `frignan`
* [source]("../../../quizzes/src/quiz02/b_frignan/mod.rs")
* A price checker for the desired token on a desired blockchain
