# To set-up a wallet, please see: [source](dapps/README.md); as this is Part 1
* "Cast Wallet"
### Once complateted move onto, Part 2
---

# How to Stand Up a New `tvá` Instance (Part 2)

This doc covers the two remaining steps: building the
workflow YML, _and_ setting the GitHub secrets/variables it needs to run.

This is written generically — no assumption about which repo, which wallet
names, or which instance you're setting up. Wherever you see `<...>`, that's
a placeholder: swap it for your own values and use the *same* value
everywhere it appears.

| Placeholder | What it means | Example |
|---|---|---|
| `<your-repo>` | the repo this workflow file and its trade log will live in | `use_your_noodle` |
| `<instance>` | a short lowercase name for this specific tva instance | `tva_uyn` |
| `<TRADING_WALLET>` | uppercase name for the wallet tva signs and trades as | `SCONE` |
| `<VAULT_WALLET>` | uppercase name for the wallet tva sends its cut to | `CHI` |

## 1. Confirmation on casting two wallets, not one

- **`<TRADING_WALLET>`** — the trading wallet. tva signs and sends *as* this
  wallet, so it needs the full Part 1 treatment: address, keystore JSON,
  keystore password.
- **`<VAULT_WALLET>`** — the Vault. tva only ever sends *to* this wallet, it
  never signs as it. Run Part 1 Steps I–III for it too, but you only need the
  **address** out of it — skip exporting a keystore JSON/password for it,
  this workflow never asks for one.
> The _two wallets_ are referring to:
>
> a wallet `tva` runs in and a _vault_ `maegen` will balance $UNDEAD with.
> 
> e.g.
> 
> 
> `tva` - has its own _address_, _keystore_json_, and _keystore_password_
> 
> `maegen` - has its own _address_, _keystore_json_, and _keystore_password_

## 2. Set these in GitHub Secrets

In `<your-repo>` (the repo this workflow file will live in):

**Settings → _Secrets_:**

| Name | What it is |
|---|---|
| `<TRADING_WALLET>_ADDRESS` | the trading wallet's public address |
| `<TRADING_WALLET>_KEYSTORE_JSON` | the trading wallet's encrypted V3 keystore JSON blob (whole file's contents) |
| `<TRADING_WALLET>_KEYSTORE_PASSWORD` | the trading wallet's keystore password from Part 1, Step II |
| `<VAULT_WALLET>_ADDRESS` | the vault wallet's public address only |
| `COMMIT_EMAIL` | git commit author email, for the trade-log auto-commit |

**Settings → _Variables_:**

| Name | What it is |
|---|---|
| `COMMIT_NAME` | git commit author name |

(`COMMIT_NAME` is a **Variable**, `COMMIT_EMAIL` is a **Secret** — different tabs in
GitHub. If your org already has a shared committer identity convention for
other automations, reuse those variable/secret names here instead of adding
new ones.)

Why these particular secret names matter: tva's compiled CLI only ever looks
for four fixed env var names — `TVA_WALLET_ADDRESS`, `TVA_KEYSTORE_PATH`,
`KEYSTORE_PASSWORD`, `VAULT_ADDRESS` — no matter which real wallet is behind a
given instance. It has no idea what you named your wallets. The YML below
maps your `<TRADING_WALLET>_*` / `<VAULT_WALLET>_*` secrets onto those four
fixed names at run time — that mapping is the one part of this file you
should *not* rename.

## 3. The workflow YML

See these files as a template `<your-repo>/.github/workflows/<instance>.yml`:
* The `YML` to run `tva`: [source](.github/workflows/tva.yml)
* The `YML` to run `maegen`: [source](.github/workflows/maegen.yml)
> You'll _need_ to revise the `YML`s to introduce _YOUR_ wallet's secrets

## 4. Run it

* Once the secrets/ variables above are set and everything within the
YML is replaced with your real values, make sure your log file is 
created before running the program. 
* Make sure you include a _checkout_ step for your repo when
building `maegen`'s `yml` on your repo
* Within the _checkout_ for `maegen` include:
```ymal
- name: Checkout trading
        uses: actions/checkout@v4
        with:
          repository: pivoteur/trading
          path: trading
```
* Trigger it once by hand from the Actions tab 
(`workflow_dispatch`, `dry_run: true` first) before letting the
hourly `cron` take over.

## 5. "Where is `tva`'s hisory?
### A good error don't worry
* This is a safey gaurd for `tva`; in other words,
_YOU_ are to open the first two pivots for this program to work.
* Please now move on to this final doc of how to manually build 
a log for `tva` to run beautifully: 
    * [source](manually-logging.md)
