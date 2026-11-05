//! Battle rules catalog tests.

use pkrs_data::{BattleRules, StatusKind};

#[test]
fn gen3_defaults_include_major_statuses() {
    let rules = BattleRules::gen3_defaults();
    assert_eq!(rules.status("burn").unwrap().kind, StatusKind::Major);
    assert!(rules.weather("sandstorm").unwrap().chip_denom.is_some());
}
