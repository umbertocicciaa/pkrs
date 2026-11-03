//! Cross-crate check: battle-side helpers use [`GameRng`] only through the port trait.
//!
//! The main assertion is that `GameRng` is object-safe (`dyn GameRng` compiles and dispatches).

use pkrs_core::{GameRng, Pcg64Rng};

fn crit_roll(rng: &mut dyn GameRng) -> bool {
    rng.gen_range_u32(100) < 6
}

#[test]
fn battle_helper_accepts_dyn_game_rng() {
    let mut rng = Pcg64Rng::from_seed(0xCAFE);
    let as_trait: &mut dyn GameRng = &mut rng;
    let _ = crit_roll(as_trait);
}
