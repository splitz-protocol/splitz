# A chain of our own

Every other lane in this repository stops at the URI. `rust/splitz-core/tests/oracle.rs`
checks that librustzcash's `zip321` parses back exactly what §8 rendered, which
is the strongest claim that can be made without a node.

This one stands up a regtest chain, funds a wallet on it, and carries one
payment request the rest of the way: **a transaction built from a URI this
library rendered is broadcast, mined, and the payee's balance read back.**

## State

Working, and observed running end to end:

- `zebrad` in regtest, no peers, every upgrade through NU5 active at height 1.
  `zebrad.toml.in` owns that list.
- Blocks on demand through the `generate` RPC. The released image is built
  without the `internal-miner` feature, so `internal_miner = true` in the
  config does nothing; `generate` is what actually mines.
- `lightwalletd` against Zebra over gRPC. There is no arm64 image, so it runs
  emulated — slower, and no difference to what is proved.
- The chain mines to an address derived from the wallet's own seed
  (`prover miner-address`), templated into `zebrad.toml` at run time so the
  two cannot drift.
- `prover prove` opens a throwaway `zcash_client_sqlite` wallet, creates the
  spending account **and the payee account** at a birthday of height 1, and
  syncs the chain through `zcash_client_backend::sync::run` over a
  compact-block cache held in memory.
- It shields the matured coinbase into Orchard: proposed, proved with the
  Sapling parameters, signed, broadcast, mined and confirmed.
- splitz renders one payer's obligation as a ZIP 321 URI, librustzcash's
  `zip321` parses it, and the wallet builds, proves, signs and broadcasts a
  transfer from what it parsed.
- The payee's Orchard balance is read either side of the transfer and the
  difference compared to the figure splitz named. **That comparison is the
  lane**: everything before it is setup. It is a difference and not a total, so
  `prove` can be run repeatedly against one chain — each run pays the same
  address again.

## Three things this has caught

Each would have reached a wallet author first.

- **The wallet's consensus parameters have to be the chain's.** Built against
  `Network::TestNetwork`, the wallet treats a 110-block chain as entirely
  pre-Sapling — testnet activates Sapling at 280,000 — so `update_chain_tip`
  records nothing and every later call fails with "chain height unknown". The
  prover uses a `LocalNetwork` matching `zebrad.toml.in`, and checks the
  branch id the node reports against the one it would build at before it
  builds anything: they are configured in two files and the first version of
  this harness had them disagree.
- **Creating an account re-queues the chain from its birthday.** A second
  account made after the sync — here, the one standing in for the payee —
  empties `block_fully_scanned`, which reads the first `Scanned` range
  starting at or below the wallet birthday and finds none. The anchor
  `get_target_and_anchor_heights` hands the input selector then falls back to
  the end of the first scan batch, every note above it is invisible to
  selection, and `propose_transfer` fails with `Insufficient balance
  (have 0, …)` while the wallet summary reports the same notes as spendable.
  Both accounts are created before `sync::run`, so one sync covers both.
- **A coinbase output cannot be spent for 100 blocks, and the input selector
  does not know that.** With a one-confirmation policy it picks the largest
  UTXO it can see, which on a freshly mined chain is an immature one, and the
  node rejects the transaction after it has been proved and signed. Shielding
  asks for 100 confirmations and refuses zero-conf shielding.

## Use

```
tools/regtest/run.sh up      # derive the miner address, start the node,
                             #   mine past coinbase maturity, start lightwalletd
tools/regtest/run.sh prove   # sync, shield, render, parse, send, check
tools/regtest/run.sh down    # tear the chain down, state and all
```

`prove` fetches the Sapling proving parameters on first use — about 51 MB,
verified against the hashes `zcash_proofs` pins, into that crate's default
parameters folder. `RUST_LOG=zcash_client_backend=info` shows what the sync is
doing.

The chain is ephemeral by design: `state.ephemeral = true`, and `down` removes
the volumes. A proof that only reproduces on a chain somebody kept is not a
proof anybody else can run. `prove` needs no teardown between runs: it opens a
throwaway wallet each time and checks a balance difference.
