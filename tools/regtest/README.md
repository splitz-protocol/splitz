# A chain of our own

Every other lane in this repository stops at the URI. `rust/tests/oracle.rs`
checks that librustzcash's `zip321` parses back exactly what §8 rendered, which
is the strongest claim that can be made without a node: **no transaction has
ever been broadcast from a URI this library produced.**

This is the harness for closing that. It stands up a regtest chain, funds a
wallet on it, and is meant to carry one payment request the rest of the way —
render, parse with somebody else's parser, build, sign, broadcast, mine, and
then read the recipient's balance, which is the only check that answers *does
the money arrive*.

## State

**Working and verified:**

- `zebrad` 6.3.0 in regtest, every upgrade active at height 1 — an Orchard
  output needs NU5, and a chain that stops at Canopy refuses one without
  saying why.
- Blocks on demand through the `generate` RPC. The released image is built
  without the `internal-miner` feature, so `internal_miner = true` in the
  config does nothing; `generate` is what actually mines.
- `lightwalletd` 0.5.4 against Zebra over gRPC. There is no arm64 image, so it
  runs emulated — slower, and no difference to what is proved.
- The chain mines to an address derived from the wallet's own seed
  (`prover miner-address`), so the coinbase lands somewhere the wallet can
  shield and then spend. The address is templated into `zebrad.toml` at run
  time rather than pinned in two places.

**Not done.** The wallet half: sync through `zcash_client_backend::sync::run`,
`shield_transparent_funds` over the matured coinbase, `propose_transfer` from
the parsed `TransactionRequest`, `create_proposed_transactions`, submit, mine,
and compare the recipient's balance against what §4 said they were owed. It
needs Sapling proving parameters on disk, which this harness does not yet
fetch. **Until that runs, the claim in `README.md` stands unchanged.**

## Use

```
tools/regtest/run.sh up      # derive the miner address, start the node,
                             #   mine past coinbase maturity, start lightwalletd
tools/regtest/run.sh prove   # not yet implemented — see State
tools/regtest/run.sh down    # tear the chain down, state and all
```

The chain is ephemeral by design: `state.ephemeral = true`, and `down` removes
the volumes. A proof that only reproduces on a chain somebody kept is not a
proof anybody else can run.
