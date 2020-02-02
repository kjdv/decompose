mod common;

use common::*;

#[test]
fn can_start_and_stop() {
    let mut f = Fixture::new("single.toml");
    f.expect_start();
    let prog = f.expect_program_starts();
    f.stop();
    f.expect_program_terminates(&prog);
    f.expect_stop();
}

#[test]
fn stop_if_all_programs_dies() {
    let mut f = Fixture::new("single.toml");
    let prog = f.expect_program_starts();
    f.terminate_program(&prog);
    f.expect_program_dies(&prog);
    f.expect_stop();
}

#[test]
fn program_is_killed_if_it_catches_sigterm() {
    let mut f = Fixture::new("diehard.toml");
    let prog = f.expect_program_starts();
    f.stop();
    f.expect_program_is_killed(&prog);
    f.expect_stop();
}
