mod common;

mod ensemble {
    use super::common::*;

    fn assert_ready(f: &mut Fixture) -> (ProgramInfo, ProgramInfo) {
        f.expect_start();

        let srv = f.expect_program_ready();
        assert_eq!("server", srv.name);

        let proxy = f.expect_program_ready();
        assert_eq!("proxy", proxy.name);

        (srv, proxy)
    }

    #[test]
    fn starts_and_stops_in_the_right_order() {
        let mut f = Fixture::new("ensemble.toml");
        let (srv, proxy) = assert_ready(&mut f);

        let body = call(9091, "hello").expect("call");
        assert_eq!("hello!\n".to_string(), body);

        //f.stop();

        //f.expect_program_terminates(&proxy);
        //f.expect_program_terminates(&srv);
        //f.expect_stop();
    }

    /*

    #[test]
    fn sets_args() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let body = call(9090, "args 1").expect("call");
        assert_eq!("extra".to_string(), body);

        f.stop();
        f.expect_stop();
    }

    #[test]
    fn sets_env() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let body = call(9090, "env FOO").expect("call");
        assert_eq!("BAR".to_string(), body);
    }

    #[test]
    fn sets_cwd() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let body = call(9090, "cwd").expect("call");
        assert!(body.ends_with("target/testrun"));
    }

    */
}
