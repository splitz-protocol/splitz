# splitz_host

What a Zcash wallet needs around `package:splitz_core`: the seam of `SPEC.md`
§15, Ed25519 signing and verification of entries, XChaCha20-Poly1305 sealing
for a relay, the bill store and its sync, where bill keys and the signing
identity live, swaps, and activity. No screens, no wallet named, no Flutter.

A wallet implements `SplitsWallet` — who it speaks as, how it sends, where its
secrets go — and every rule of the protocol comes from `splitz_core`.

See the [repository](https://github.com/splitz-protocol/splitz) for
`SPEC.md`, `INTEGRATING.md` and the tests.
