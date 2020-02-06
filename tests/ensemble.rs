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

#[test]
fn sets_args() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "args?idx=1");
    assert_eq!(200, status);
    assert_eq!("extra", body);

    f.stop();
    f.expect_stop();
}


#[test]
fn sets_env() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "env?key=FOO");
    assert_eq!(200, status);
    assert_eq!("BAR", body);
}

#[test]
fn sets_cwd() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "cwd");
    assert_eq!(200, status);
    assert!(body.ends_with("target/testrun"));
}
