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

        f.stop();

        f.expect_program_terminates(&proxy);
        f.expect_program_terminates(&srv);
        f.expect_stop();
    }

    #[test]
    fn sets_args() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let body = call(9090, "args?idx=1").expect("call");
        assert_eq!("extra".to_string(), body);

        f.stop();
        f.expect_stop();
    }

    #[test]
    fn sets_env() {
        std::env::set_var("DYN_FOO", "dynamic");

        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let foo = call(9090, "env?key=FOO").expect("call");
        assert_eq!("BAR".to_string(), foo);

        let dyn_foo = call(9090, "env?key=DYN_FOO").expect("call");
        assert_eq!("dynamic".to_string(), dyn_foo);

        let def_foo = call(9090, "env?key=DEF_FOO").expect("call");
        assert_eq!("default".to_string(), def_foo);
    }

    #[test]
    fn sets_cwd() {
        let mut f = Fixture::new("ensemble.toml");
        assert_ready(&mut f);

        let body = call(9090, "cwd").expect("call");
        assert!(body.ends_with("target/testrun"));
    }
}
