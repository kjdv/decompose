mod common;

use common::*;

#[test]
fn can_start() {
    let f = Fixture::new("single.toml");
    f.expect_start();
}
