# ceap

Trades one token for another on a given blockchain, via KyberSwap.

Usage: ceap [OPTIONS] --wallet-address <WALLET_ADDRESS> --keystore-path <KEYSTORE_PATH> <FROM_TOKEN> <AMOUNT> <TO_TOKEN>

> n.b. if the trade fails, consider setting `--slippage-bps` to 100 (1%)

Arguments:
  <FROM_TOKEN>
          trading from this token

  <AMOUNT>
          amount of `from_token` to trade

  <TO_TOKEN>
          trading to this token

Options:
      --floor <FLOOR>
          Minimum acceptable output amount
          
          [default: 0]

      --slippage-bps <SLIPPAGE_BPS>
          Slippage percent (where 1000 BPS is 10% higher than floor)
          
          [default: 0]

      --wallet-address <WALLET_ADDRESS>
          wallet address on which trade occurs
          
          [env: WALLET_ADDRESS=]

      --keystore-path <KEYSTORE_PATH>
          Keystore to wallet: permits trades on this wallet
          
          [env: KEYSTORE_PATH]

      --blockchain <BLOCKCHAIN>
          blockchain on which to execute trade
          
          [default: AVALANCHE]

      --dry-run <DRY_RUN>
          Force a dry run
          
          [default: LIVE]

  -d, --debug
          Show debugging information

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

## Examples

```BASH
$ ceap BTC 1 ETH
$ ceap BTC 1 USDT --floor 63000 --blockchain binance
```

## Requirements

- A `<blockchain>.toml` token registry file must exist in `data/tokens` in
this repository

When `--dry-run` selected  nothing moves

-----

## Revision history

* 1.2.3, 2026-10-22: Handle native token swaps in library code
* 1.2.2, 2026-10-20: show what would be the trade-amounts, even during `DRYRUN`;
`mock_wallet` is now `async`
* 1.2.1, 2026-10-12: moved most of libs to implementation-level
* 1.2.0, 2026-10-08: shift to `Wallet` implementation; make DRY_RUN enum
* 1.1.5, 2026-09-21: externalized token-registry
* 1.1.4, 2026-09-20: reduced trade amounts to f32-size
* 1.1.3, 2026-09-18: directory-restructuring
* 1.1.2, 2026-09-18: parameterize `slippage_bps`
* 1.1.1, 2026-09-17: allow comma-floats as inputs to `floor` and `amount`
* 1.1.0, 2026-09-15: library-refactoring
* 1.0.0, 2026-08-20: released

-----

## Verifications

| blockchain | from   | amt       | to     | received | date       | tx_id   |
| :--------- | :----- | --------: | :----- | -------: | :-----     | :-----: |
| Avalanche  | UNDEAD | 70,000.00 | USDC   | 24.37    | 2026-10-21 | [https://snowtrace.io/tx/0x1d83...1061](https://snowtrace.io/tx/0x1d830b179a37ed61c3816aa5cf57d5bd58888071aa14ff3b1b95952e1774106a) |
| Avalanche  | AVAX   | 0.01      | UNDEAD | 2662.75  | 2026-10-22 | [https://snowtrace.io/tx/0x8486...5582](https://snowtrace.io/tx/0x8486ecd47658980ab94d1532a7af78d0c7722e666f98a6f14e651c4ad1035582) |
| Avalanche  | USDC   | 5.00 | AVAX | 0.54 | 2026-10-22 | [https://snowtrace.io/tx/0xe70fb...82cf](https://snowtrace.io/tx/0xe70fb045d9916f409fe6b8ab9c15f2e7b921be94f1503acf639916b10b8282cf) |
| Binance    | DOGE | 45.00 | USDt   | 3.74    | 2026-10-21 | [https://bscscan.com/tx/0xcd9c...bf1f](https://bscscan.com/tx/0xcd9c1ef96818d2fb023d5c844c892531af8e6b339e13e8c9a790c3663335bf1f)
| Binance  | BNB   | 0.05 | USDt | 36.39      | 2026-10-22 | [https://bscscan.com/tx/0x5e83...05cf](https://bscscan.com/tx/0x5e83591eaad935db3a2180ede43a6034c1204f77c11c2ab0a6a66a22828605cf) |
| Binance | USDt | 1.00 | BNB | 0.0013 | 2026-10-22 | [https://bscscan.com/tx/0x9d49...157a](https://bscscan.com/tx/0x9d49904a074a936a783e8160b8ba716718e1fddef3b38b2de1979c3a788d157a) |
