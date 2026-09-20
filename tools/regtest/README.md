# A chain of our own

Every other lane in this repository stops at the URI. `rust/tests/oracle.rs`
checks that librustzcash's `zip321` parses back exactly what §8 rendered, which
is the strongest claim that can be made without a node: **no transaction has
ever been broadcast from a URI this library produced.**

This is the harness for closing that. It stands up a regtest chain, funds a
wallet on it, and carries one payment request as far as it currently goes.

## State

**Working, and observed running:**

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
- `prover prove` opens a throwaway `zcash_client_sqlite` wallet, creates an
  account at a birthday of height 1, and syncs the chain through
  `zcash_client_backend::sync::run` over a compact-block cache held in memory.
  It sees the coinbase.
- It shields the matured coinbase into Orchard: proposed, proved with the
  Sapling parameters, signed, **broadcast to the node, mined, and confirmed**,
  with the resulting Orchard balance read back afterwards.
- splitz renders one payer's obligation as a ZIP 321 URI and **librustzcash's
  `zip321` parses it**, on a real unified address of the chain the transaction
  would go to.

**Where it stops.** `propose_transfer` cannot select a note. The wallet's
`block_fully_scanned()` stays `None` however many times the chain is scanned:
`suggest_scan_ranges` keeps returning one `Historic` range covering the whole
chain, so the anchor handed to the input selector sits at the end of the first
scan batch and every note above it is invisible to selection. The failure
surfaces as `Insufficient balance (have 0, …)` while the same notes are
reported as spendable in the wallet summary. `prove` checks for this before
proposing and says so rather than letting the misleading message stand.

Two things not yet ruled out: that the in-memory `BlockCache` in `src/cache.rs`
is at fault, and that a chain too short for lightwalletd to report any
completed subtree root (`get_subtree_roots` returns none here) leaves the
commitment tree unable to satisfy the scan queue. **Until a transfer built from
a splitz URI is mined and the payee's balance read, the claim in `README.md`
stands unchanged.**

## Two things this already caught

Both would have reached a wallet author first.

- **The wallet's consensus parameters have to be the chain's.** Built against
  `Network::TestNetwork`, the wallet treats a 110-block chain as entirely
  pre-Sapling — testnet activates Sapling at 280,000 — so `update_chain_tip`
  records nothing and every later call fails with "chain height unknown". The
  prover uses a `LocalNetwork` matching `zebrad.toml.in`, and checks the
  branch id the node reports against the one it would build at before it
  builds anything: they are configured in two files and the first version of
  this harness had them disagree.
- **A coinbase output cannot be spent for 100 blocks, and the input selector
  does not know that.** With a one-confirmation policy it picks the largest
  UTXO it can see, which on a freshly mined chain is an immature one, and the
  node rejects the transaction after it has been proved and signed. Shielding
  asks for 100 confirmations and refuses zero-conf shielding.

## Use

```
tools/regtest/run.sh up      # derive the miner address, start the node,
                             #   mine past coinbase maturity, start lightwalletd
tools/regtest/run.sh prove   # sync, shield, render, parse — see State
tools/regtest/run.sh down    # tear the chain down, state and all
```

`prove` fetches the Sapling proving parameters on first use — about 51 MB,
verified against the hashes `zcash_proofs` pins, into that crate's default
parameters folder. `RUST_LOG=zcash_client_backend=info` shows what the sync is
doing.

The chain is ephemeral by design: `state.ephemeral = true`, and `down` removes
the volumes. A proof that only reproduces on a chain somebody kept is not a
proof anybody else can run.
