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

Save this as `<your-repo>/.github/workflows/<instance>.yml`:

```yaml
name: <instance>

on:
  workflow_dispatch:
    inputs:
      dry_run:
        description: 'Dry run (no funds moved, no log entries written)?'
        type: boolean
        default: false
      debug:
        description: 'Debug mode (verbose logging)?'
        type: boolean
        default: false
      blockchain:
        description: 'Which chain (data/{blockchain}.toml) to trade against?'
        type: string
        default: 'avalanche'
      btc_trade_amount:
        description: 'BTC amount to open a new BTC->UNDEAD position with each cycle'
        type: string
        default: '0.001'
      undead_trade_amount:
        description: 'UNDEAD amount to open a new UNDEAD->BTC position with each cycle'
        type: string
        default: '100000'
      log_path:
        description: 'Full trade log path. Leave blank for the default (<your-repo>/data/<instance>-trades.log under the runner workspace).'
        type: string
        default: ''
  schedule:
    - cron: "0 * * * *"

concurrency:
  group: <instance>
  cancel-in-progress: false

env:
  DAPP: tva

permissions:
  contents: write

jobs:
  run-<instance>:
    runs-on: ubuntu-latest

    steps:
      - name: Checkout <your-repo>
        uses: actions/checkout@v4
        with:
          path: <your-repo>

      - name: Checkout trading
        uses: actions/checkout@v4
        with:
          repository: pivoteur/trading
          path: trading

      - name: Checkout protocol
        uses: actions/checkout@v4
        with:
          repository: pivoteur/protocol
          path: protocol
          ref: main

      - name: Load Shared Environment Variables
        uses: doughepi/yaml-env-action@v1.0.0
        with:
          files: trading/.github/config/trading-env.yml

      - name: Establish app path
        run: echo "DAPP_DIR=$DAPPS_DIR/$DAPP" >> $GITHUB_ENV

      - name: Normalize workflow inputs
        # workflow_dispatch defaults do NOT apply on the schedule trigger --
        # every later step reads only these env.* values, never raw
        # inputs.*, so a manual run and a scheduled run resolve identically.
        run: |
          echo "DRY_RUN=${{ github.event.inputs.dry_run || 'false' }}" >> $GITHUB_ENV
          echo "DEBUG=${{ github.event.inputs.debug || 'false' }}" >> $GITHUB_ENV
          echo "BLOCKCHAIN=${{ github.event.inputs.blockchain || 'avalanche' }}" >> $GITHUB_ENV
          echo "BTC_TRADE_AMOUNT=${{ github.event.inputs.btc_trade_amount || '0.001' }}" >> $GITHUB_ENV
          echo "UNDEAD_TRADE_AMOUNT=${{ github.event.inputs.undead_trade_amount || '100000' }}" >> $GITHUB_ENV
          echo "LOG_PATH_INPUT=${{ github.event.inputs.log_path || '' }}" >> $GITHUB_ENV

      - name: Establish trade log path
        run: echo "TVA_LOG_PATH=${LOG_PATH_INPUT:-$GITHUB_WORKSPACE/<your-repo>/data/<instance>-trades.log}" >> $GITHUB_ENV

      - name: Cache tva binary
        id: binary-cache
        uses: actions/cache@v4
        with:
          path: trading/dapps/tva/target/release/tva
          key: >-
            <instance>-binary-
            ${{ hashFiles('<path/to/your/tva/source>/**/*.rs') }}-
            ${{ hashFiles('trading/libs/src/auto_trading.rs') }}-
            ${{ hashFiles(format('{0}/Cargo.toml', env.DAPP_DIR)) }}-
            ${{ hashFiles(format('{0}/Cargo.lock', env.DAPP_DIR)) }}
        # <path/to/your/tva/source> must point at wherever YOUR tva
        # instance's own Rust source actually lives -- if this doesn't
        # match the real folder, edits there will never bust the cache
        # and this workflow will keep running a stale binary.

      - name: Install Rust toolchain
        if: steps.binary-cache.outputs.cache-hit != 'true'
        uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: stable

      - name: Checkout crypto-n-rust
        if: steps.binary-cache.outputs.cache-hit != 'true'
        uses: actions/checkout@v4
        with:
          repository: logicalgraphs/crypto-n-rust
          path: crypto-n-rust

      - name: Show input parameters
        run: |
          echo "=== Running <instance> with resolved inputs (manual or scheduled) ==="
          echo "dry_run: $DRY_RUN"
          echo "debug: $DEBUG"
          echo "blockchain: $BLOCKCHAIN"
          echo "btc_trade_amount: $BTC_TRADE_AMOUNT"
          echo "undead_trade_amount: $UNDEAD_TRADE_AMOUNT"
          echo "log_path: $TVA_LOG_PATH"

      - name: Build tva
        if: steps.binary-cache.outputs.cache-hit != 'true'
        run: |
          cargo build --manifest-path $DAPP_DIR/Cargo.toml --bin $DAPP --release

      - name: Add tva to PATH
        run: echo "$GITHUB_WORKSPACE/$DAPP_DIR/$EXE_SUBDIR" >> $GITHUB_PATH

      - name: Write keystore file from secret
        env:
          TRADING_KEYSTORE_JSON: ${{ secrets.<TRADING_WALLET>_KEYSTORE_JSON }}
        run: |
          if [ -z "$TRADING_KEYSTORE_JSON" ]; then
            echo "::error::<TRADING_WALLET>_KEYSTORE_JSON secret is empty -- refusing to write an empty keystore file. Set it in this repo's secrets before running this workflow."
            exit 1
          fi
          echo "$TRADING_KEYSTORE_JSON" > "$RUNNER_TEMP/keystore.json"
          echo "TVA_KEYSTORE_PATH=$RUNNER_TEMP/keystore.json" >> $GITHUB_ENV

      - name: Run tva
        # tva's clap args resolve their env fallbacks under fixed names
        # baked into the compiled binary (TVA_WALLET_ADDRESS,
        # TVA_KEYSTORE_PATH, KEYSTORE_PASSWORD, VAULT_ADDRESS) regardless
        # of which wallet is backing this run -- so this instance's own
        # secret names get mapped onto those fixed names here, not renamed.
        env:
          TVA_WALLET_ADDRESS: ${{ secrets.<TRADING_WALLET>_ADDRESS }}
          KEYSTORE_PASSWORD:  ${{ secrets.<TRADING_WALLET>_KEYSTORE_PASSWORD }}
          VAULT_ADDRESS:      ${{ secrets.<VAULT_WALLET>_ADDRESS }}
        run: |
          ARGS=""
          if [ "$DRY_RUN" = "true" ]; then
            ARGS="$ARGS --dry-run"
          fi
          if [ "$DEBUG" = "true" ]; then
            ARGS="$ARGS --debug"
          fi
          if [ -n "$BLOCKCHAIN" ]; then
            ARGS="$ARGS --blockchain $BLOCKCHAIN"
          fi
          if [ -n "$BTC_TRADE_AMOUNT" ]; then
            ARGS="$ARGS --btc-trade-amount $BTC_TRADE_AMOUNT"
          fi
          if [ -n "$UNDEAD_TRADE_AMOUNT" ]; then
            ARGS="$ARGS --undead-trade-amount $UNDEAD_TRADE_AMOUNT"
          fi
          if [ -n "$TVA_LOG_PATH" ]; then
            ARGS="$ARGS --log-path $TVA_LOG_PATH"
          fi
          ARGS="$ARGS div --pct 25"   # adjust 25 to whatever % you want auto-sent to the vault
          echo "=== Running tva with args: $ARGS ==="
          cd <your-repo>
          $DAPP $ARGS

      - name: Show new trade log lines (safety net if the commit step doesn't run)
        # if: always() so this still runs even if "Run tva" failed or a
        # later step blows up before the commit step gets there -- a row
        # that was really appended to the log file must never only exist
        # on the runner's disk, which is gone once the job ends.
        if: always()
        run: |
          cd <your-repo>
          NEW_LINES=$(git diff --unified=0 -- "$TVA_LOG_PATH" | grep -E '^\+[^+]' || true)
          if [ -n "$NEW_LINES" ]; then
            echo "=== New trade log line(s) this run (not yet committed) ==="
            echo "$NEW_LINES"
          else
            echo "No new trade log lines this run."
          fi

      - name: Delete keystore file
        if: always()
        run: rm -f "$RUNNER_TEMP/keystore.json"

      - name: Commit trade log to <your-repo>
        # Explicit success() so a failure earlier in the job can never be
        # masked by a future edit to this condition -- only commit on a
        # clean, non-dry-run run.
        if: ${{ success() && env.DRY_RUN != 'true' }}
        env:
          COMMIT_NAME:  ${{ vars.COMMIT_NAME }}
          COMMIT_EMAIL: ${{ secrets.COMMIT_EMAIL }}
        run: |
          cd <your-repo>
          git config --local user.name "$COMMIT_NAME"
          git config --local user.email "$COMMIT_EMAIL"
          git add "$TVA_LOG_PATH"

          if git diff --staged --quiet; then
            echo "Nothing to commit, skipping push"
            exit 0
          fi

          git commit -m "<instance> trade log update [skip ci]"
          git push
```

## 4. Run it

Once the secrets/variables above are set and every `<...>` placeholder in the
YML is replaced with your real values, trigger it once by hand from the
Actions tab (`workflow_dispatch`, `dry_run: true` first) before letting the
hourly `cron` take over.
