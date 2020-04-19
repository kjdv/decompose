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

        let (status, body) = http_get(9091, "hello");
        assert_eq!(200, status);
        assert_eq!("hello!\n", body);

        f.stop();

        f.expect_program_terminates(&proxy);
        f.expect_program_terminates(&srv);
        f.expect_stop();
    }

    #[test]
    fn sets_args() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let (status, body) = http_get(9090, "args?idx=1");
        assert_eq!(200, status);
        assert_eq!("extra", body);

        f.stop();
        f.expect_stop();
    }

    #[test]
    fn sets_env() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let (status, body) = http_get(9090, "env?key=FOO");
        assert_eq!(200, status);
        assert_eq!("BAR", body);
    }

    #[test]
    fn sets_cwd() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let (status, body) = http_get(9090, "cwd");
        assert_eq!(200, status);
        assert!(body.ends_with("target/testrun"));
    }
}
