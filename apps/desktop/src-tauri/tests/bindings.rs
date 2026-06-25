//! Keeps the committed `src/bindings.ts` in lockstep with what ttipc
//! generates from the `cmd`/`sync` procedures and their events, using
//! ttipc's own `check` drift guard. Regenerate with `REGEN_BINDINGS=1
//! cargo test`; a plain `cargo test` fails on drift, which is the CI
//! guard against hand-edits and stale bindings.

const BINDINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts");

#[test]
fn bindings_match_generator() {
    let bindings = lux_lib::ttipc_bindings();
    if std::env::var_os("REGEN_BINDINGS").is_some() {
        bindings.export_to(BINDINGS).expect("write bindings.ts");
        return;
    }
    bindings
        .check(BINDINGS)
        .expect("src/bindings.ts is stale -- regenerate with `REGEN_BINDINGS=1 cargo test`");
}
