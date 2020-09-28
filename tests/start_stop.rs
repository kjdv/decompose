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

    #[test]
    fn errors_on_start_timeout() {
        let mut f = Fixture::new("timeout.yaml");
        f.expect_exited();
    }

    #[test]
    fn critical_tears_down_system() {
        let mut f = Fixture::new("critical.toml");

        let srv = f.expect_program_ready();
        assert_eq!("server", srv.name);

        let task = f.expect_program_ready();
        assert_eq!("task", task.name);

        f.expect_program_dies(&task);
        f.expect_program_terminates(&srv);
        f.expect_stop();
    }

    #[test]
    fn critical_tears_down_system_for_completed_task() {
        let mut f = Fixture::new("critical_complete.toml");

        let srv = f.expect_program_ready();
        assert_eq!("server", srv.name);

        let task = f.expect_program_ready();
        assert_eq!("task", task.name);

        f.expect_program_dies(&task);
        f.expect_program_terminates(&srv);
        f.expect_stop();
    }
}
