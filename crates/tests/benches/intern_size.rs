//! Size comparison for pojoc's `intern` string-dedup feature: a large array
//! of strings drawn from a small pool of repeated values (the case `intern`
//! is built for), compared against protobuf/Cap'n Proto/FlatBuffers/Bebop —
//! none of which dedup repeated string values, so they scale linearly with
//! element count regardless of how few distinct strings are actually present.
//!
//! `pojoc (plain)` re-encodes the identical data through a non-interned
//! `[string]` field in the same schema, isolating interning's contribution
//! from pojoc's general wire-format overhead.

use bebop::Record;
use flatbuffers::FlatBufferBuilder;
use pojoc_tests::{
    fb_intern_bench, intern_bench_bebop, intern_bench_capnp,
    pojoc_intern_bench::{self, InternBench, runtime::*},
    proto_intern_bench,
};
use prost::Message;

const POOL: &[&str] = &[
    "starter_zone",
    "pvp_enabled",
    "vip",
    "quest_giver",
    "faction_red",
    "faction_blue",
    "trade_hub",
    "safe_zone",
    "pve_only",
    "event_active",
    "guild_hall",
    "raid_boss",
    "dungeon_entrance",
    "world_boss",
    "crafting_station",
    "auction_house",
];
const ELEMENT_COUNT: usize = 5000;

fn pool_cycle() -> Vec<&'static str> {
    (0..ELEMENT_COUNT).map(|i| POOL[i % POOL.len()]).collect()
}

fn main() {
    let values = pool_cycle();

    // Only `tags` (interned) populated — `tags_plain` stays empty so its
    // own (near-zero) wire cost doesn't get counted into this measurement.
    let interned_only = InternBench {
        tags: PojocVec::from_vec(values.clone()),
        tags_plain: PojocVec::new(),
    };
    let mut pojoc_interned_buf = Vec::new();
    pojoc_intern_bench::encode(&mut pojoc_interned_buf, &interned_only);

    // Same data through the non-interned `[string]` field instead — isolates
    // interning's own contribution from pojoc's general wire-format overhead.
    let plain_only = InternBench {
        tags: PojocVec::new(),
        tags_plain: PojocVec::from_vec(values.clone()),
    };
    let mut pojoc_plain_buf = Vec::new();
    pojoc_intern_bench::encode(&mut pojoc_plain_buf, &plain_only);

    let proto_msg = proto_intern_bench::InternBench {
        tags: values.iter().map(|s| s.to_string()).collect(),
    };
    let proto_buf = proto_msg.encode_to_vec();

    let mut capnp_msg = capnp::message::Builder::new_default();
    {
        let mut root = capnp_msg.init_root::<intern_bench_capnp::intern_bench::Builder>();
        let mut tags = root.reborrow().init_tags(values.len() as u32);
        for (i, s) in values.iter().enumerate() {
            tags.set(i as u32, s);
        }
    }
    let mut capnp_buf = Vec::new();
    capnp::serialize::write_message(&mut capnp_buf, &capnp_msg).unwrap();

    let mut fb_builder = FlatBufferBuilder::with_capacity(ELEMENT_COUNT * 16);
    let tags_vec = {
        let items: Vec<_> = values.iter().map(|s| fb_builder.create_string(s)).collect();
        fb_builder.create_vector(&items)
    };
    let fb_bench = fb_intern_bench::InternBench::create(
        &mut fb_builder,
        &fb_intern_bench::InternBenchArgs {
            tags: Some(tags_vec),
        },
    );
    fb_builder.finish(fb_bench, None);
    let fb_buf = fb_builder.finished_data().to_vec();

    let bebop_msg = intern_bench_bebop::InternBench {
        tags: Some(values.clone()),
    };
    let mut bebop_buf = Vec::new();
    bebop_msg.serialize(&mut bebop_buf).unwrap();

    println!(
        "\n=== interned string array: {ELEMENT_COUNT} elements from a {}-string pool ===",
        POOL.len()
    );
    println!("POJOC (intern):   {} bytes", pojoc_interned_buf.len());
    println!(
        "POJOC (plain []): {} bytes  <- same data, no dedup (isolates interning's own contribution)",
        pojoc_plain_buf.len()
    );
    println!("Protobuf:         {} bytes", proto_buf.len());
    println!("Cap'n Proto:      {} bytes", capnp_buf.len());
    println!("FlatBuffers:      {} bytes", fb_buf.len());
    println!("Bebop:            {} bytes", bebop_buf.len());
    println!(
        "\npojoc (intern) is {:.1}x smaller than the next smallest competitor",
        [
            proto_buf.len(),
            capnp_buf.len(),
            fb_buf.len(),
            bebop_buf.len()
        ]
        .into_iter()
        .min()
        .unwrap() as f64
            / pojoc_interned_buf.len() as f64
    );
    println!("=====================================================================\n");
}
