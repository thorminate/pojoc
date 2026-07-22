# Security notes

## Hash-flooding resistance in `InternBuilder` and `PojocMap`

Two runtime hash maps sit on data that can originate from the wire:

- `InternBuilder` (`crates/runtime/src/intern.rs`) dedups strings for
  `intern`-marked fields during encode.
- `PojocMap<K, V>` (`crates/runtime/src/lib.rs`), the type backing `map<K, V>`
  schema fields, is built up entry-by-entry via `.insert()` while decoding —
  this is the more exposed of the two, since any schema with a
  `map<string, ...>` field gets attacker-controllable keys straight off a
  socket, no relay or re-encode required.
- `InternBuilder` is reachable by untrusted input only indirectly: a service
  that decodes an untrusted message and re-encodes it (a relay, gateway, or
  anything that round-trips external data) into `intern`-marked fields.

Both are keyed with a random per-process seed
([`PojocHasher`](../crates/runtime/src/lib.rs)), so an attacker can't
precompute a batch of colliding keys offline and ship a payload that
degrades every instance's hash map to O(n) lookups. Each process picks its
own seed at startup, so a precomputed collision set only works if the
attacker can recover *that* seed — which requires sustained interaction with
one running process (e.g. a timing side-channel), not a one-shot crafted
payload.

`PojocHasher` is [`foldhash::fast::RandomState`] by default (the `foldhash`
crate feature, on by default). `foldhash` is not a cryptographic hash and
carries no formal collision-resistance proof; it trades that for being
materially cheaper per hash than SipHash. That's a fine tradeoff for the
threat model above. It would not be a fine tradeoff somewhere an attacker
gets sustained oracle access to a single long-lived process and seed
recovery over time becomes plausible — if either map ever sits behind that
kind of exposure, disable the `foldhash` feature, which falls `PojocHasher`
back to std's SipHash-based `RandomState`.

[`foldhash::fast::RandomState`]: https://docs.rs/foldhash/latest/foldhash/fast/type.RandomState.html
