mod common;

use common::*;

#[test]
fn starts_and_stops_in_the_right_order() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let srv = f.expect_program_starts();
    assert_eq!("server", srv.name);

    let proxy = f.expect_program_starts();
    assert_eq!("proxy", proxy.name);

    f.stop();

    f.expect_program_terminates(&proxy);
    f.expect_program_terminates(&srv);
    f.expect_stop();
}
