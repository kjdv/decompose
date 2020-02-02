mod common;

use common::*;

#[test]
fn can_start() {
    let mut f = Fixture::new("single.toml");
    f.expect_start();
}
