**This project is currently a work in progress. It is provided as-is, without
any warranty of correctness, functionality, or fitness for any particular
purpose. There is no guarantee that it works as intended, and it may contain
bugs, incomplete features, or incorrect cryptographic behavior.**

**Do not use this software for security-critical or production purposes. Use at
your own risk.**

# Shadow Harvester

An application that uses AshMaize PoW algorithm to scavenge for night.

# Building from source with 'cargo', if you don't have nix


```bash
cargo build --release
```

If you encounter issue with the build try to run `cargo  fix --bin "shadow-harvester"`

Run

```bash
cargo run --release -- <args>
```
OR 


```bash
./target/release/shadow-harvester <args>
```

# Usage
The below commands will use `nix` for running the command, if you don't use nix you can use `cargo` or just build `./target/release/shadow-harvester <args>`


## Running with a Mnemonic File

To run Shadow Harvester with a mnemonic file:

```bash
nix run .\#shadow-harvester -- --api-url https://scavenger.prod.gd.midnighttge.io --accept-tos --mnemonic-file wallet.mnemonic
```

## Creating a Wallet

If you haven't created a wallet yet, you can generate a recovery phrase using `cardano-address`:

```bash
cardano-address recovery-phrase generate > wallet.mnemonic
```

**Security Note:** There is always a security risk when using plain text mnemonics. For  Midnight, it is recommended that you transfer your NIGHT tokens to your preferred secure wallets after the glacier drop period concludes.

## Running with a Single Payment Key

If you just want to mine with a single key:

```bash
nix run .\#shadow-harvester -- --api-url https://scavenger.prod.gd.midnighttge.io --accept-tos --payment-key "YOURED25519PRIVATEKEY"
```

**Note on Ephemeral Keys**

Ephemeral keys are not recommended for use. Currently, the donate-to endpoint is not active, which means any keys generated ephemerally are discarded and never persisted to disk. While this approach was initially considered, the implementation was switched to mnemonic-based keys due to the non-functional donate-to endpoint. Use mnemonic files or payment keys instead until the donate-to functionality becomes available.

# License

This project is licensed under either of the following licenses:

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)
