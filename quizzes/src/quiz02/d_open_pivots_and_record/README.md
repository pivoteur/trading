# `d_open_pivots_and_record`

We have the assets available, so now we open new pivots (constrainted by some 
maximum amount).

Now 'we open new pivots' is a rather facile statement that I feel must needs
some clarification, because, after all, virtual pivots are superior to actual
pivots.

Why?

because virtual pivots can be adjusted by virtsz ... that's why!

So:

* commit to a virtual pivot, if possible*
* commit to an actual pivot, if possible
* bid you the fondest of adieu

Committing the assets is one thing, computing the pivot then writing it out 
the pivot-table is another thing.

We need to do both, because we need to know what assets are committed for the 
next pivot-call.

To do the above

* We'll use [ceap](../../../../dapps/ceap) to execute the trades to open the
pivots
* We also need to record the new opened pivots in the open pivot table, using
[protocol repository's
libs](https://github.com/pivoteur/protocol/tree/main/libs)

Of course, eventually, we'll need to close the opened pivots, but that's the
next dapp. We need opened pivots in order to close pivots, so let's get'r done!

