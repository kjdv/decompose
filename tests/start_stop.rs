mod common;

mod start_stop {
    use super::common::*;

    #[test]
    fn can_start_and_stop() {
        let mut f = Fixture::new("single.toml");
        f.expect_start();
        f.expect_program_starts();
        let prog = f.expect_program_ready();
        f.stop();
        f.expect_program_terminates(&prog);
        f.expect_stop();
    }

    #[test]
    fn stop_if_all_programs_die() {
        let mut f = Fixture::new("single.toml");
        f.expect_start();

        let prog = f.expect_program_ready();
        f.terminate_program(&prog);
        f.expect_program_dies(&prog);
        f.expect_stop();
    }

    #[test]
    fn program_is_killed_if_it_catches_sigterm() {
        let mut f = Fixture::new("diehard.toml");
        f.expect_start();

        let prog = f.expect_program_ready();
        f.stop();
        f.expect_program_is_killed(&prog);
        f.expect_stop();
    }

    /*
    #[test]
    fn errors_on_start_timeout() {
        let mut f = Fixture::new("timeout.yaml");
        f.expect_exited();
    }
    */
}
