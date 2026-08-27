# dapps
Contains the decentralized applications to automate trades.

## `tvá`
 * An auto-trader that pivots `$BTC` and `$UNDEAD` every hour.

## `arbitrage`
 * An all-or-nothing auto-trader for calls.csv.

## `maegen`
 * A prorgam tired to a specified wallet and balances a token
 passed in with `$UNDEAD`, balancing the dollar value that is.

## `sendan` 
 * A simple prorgam that send a desired amount of token x to a
 wallet passed in.

## `frignan`
 * A price checker for the desired token on a desired blockchain.

## `ceap`
 * An auto-trader that trades any token to any token at a specified 
 amount and on any blockchain, all passed in. 

-----

# Cast Wallet

To be able to run these auto-traders you'd need to set-up the digital 
wallet that you'd want to use.

## Step I
 * Inside your terminal, run: `cast --version`.
 * If not found, you'd need to run: `foundryup` or `curl -L https://foundry.paradigm.xyz | bash`
 to install foundry. This is needs for something called 'keystore'.
## Step II
Once installed, run: `cast wallet import <name> --interactive` Where: `<name>` is the name of your wallet. 
> (this assumes that a digital wallet aready exists somewhere, e.g. MetaMask)
 * This will prompt you with two interactions:
 1. "Enter private key:" 
    - This is from your digital wallet's address.
    - Inside of _MetaMask_ → _account menu_ → _Account details_ → _Show private key_ → _enter your MetaMask password_ → copy it and paste it within the interaction.
 2. "Enter keystore password:" 
    - This is a password that you set for the program to do the trades without human input. (This is inportant for _Step III_, save it)
    - And, this writes the encrypted V3 keystore JSON to `~/.foundry/keystores/<name>`.
    - Ensure no typos as you cannot physically see what the characters are.
## Step III
Verify all dependencies are set by running these commands:
 * `cast wallet address --account <name>` to confirm your wallet's address. This is the first secret for GitHub.
 * `cat ~/.foundry/keystores/<name>` to confirm and produce the JSON blob you need as one of the secrets. This the second secret for GitHub.
 * The "_Enter keystore password_" you set is the third secret for GitHub.
## Step IV
 * Add these three dependencies to your GitHub's repository secrets.
 * export the `wallet address` and the `~/.foundry/keystores/<name>` path, locally within `~/.bashrc`
