mod common;

mod readysignals {
    use super::common::*;

    #[test]
    fn nothing() {
        let mut f = Fixture::new("rs_nothing.yaml");
        let prog = f.expect_program_ready();
        assert_eq!("prog", prog.name.as_str());
    }

    #[test]
    fn manual() {
        let mut f = Fixture::new("rs_manual.yaml");
        let prog = f.expect_program_starts();

        f.expect_line(format!("Manually waiting for {}, press enter", prog.name).as_str());

        f.send_stdin("\n");

        let prog = f.expect_program_ready();
        assert_eq!("prog", prog.name.as_str());
    }

    #[test]
    fn timer() {
        let mut f = Fixture::new("rs_timer.yaml");
        let prog = f.expect_program_ready();
        assert_eq!("prog", prog.name.as_str());
    }

    #[test]
    fn port() {
        let mut f = Fixture::new("rs_port.yaml");
        f.expect_program_ready();

        let status = call(9093, "hello");
        assert!(status.is_ok());
    }

    #[test]
    fn stdout() {
        let mut f = Fixture::new("rs_stdout.yaml");
        f.expect_program_ready();
    }

    #[test]
    fn stderr() {
        let mut f = Fixture::new("rs_stderr.yaml");
        f.expect_program_ready();
    }

    #[test]
    fn completed() {
        let mut f = Fixture::new("rs_completed.yaml");

        let prog = f.expect_program_ready();
        assert_eq!("dep", prog.name.as_str());

        let prog = f.expect_program_ready();
        assert_eq!("prog", prog.name.as_str());

        let status = call(9093, "health");
        assert!(status.is_ok());
    }
}
