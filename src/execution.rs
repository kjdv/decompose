extern crate string_error;
extern crate subprocess;

use log;
use signal_hook::{iterator::Signals, SIGCHLD, SIGINT, SIGTERM};
use std::error::Error;
use subprocess::{Exec, Popen, Redirection};

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Execution {
    programs: Vec<Program>,
    terminate_timeout: std::time::Duration,
}

impl Execution {
    pub fn from_config(
        cfg: config::System,
        output: output::OutputFileFactory,
    ) -> Result<Execution> {
        log::info!("starting execution");

        let mut list = executionlist::ExecutionList::from_system(&cfg, output);

        let sleep_time = std::time::Duration::from_millis(10);
        while !list.poll()? {
            std::thread::sleep(sleep_time);
        }

        let execution = Execution {
            programs: list.reap(),
            terminate_timeout: std::time::Duration::from_secs_f64(cfg.terminate_timeout),
        };

        Ok(execution)
    }

    pub fn wait(&mut self) {
        log::debug!("waiting for SIGTERM");

        let signals = Signals::new(&[SIGINT, SIGTERM, SIGCHLD]).unwrap();

        for sig in signals.forever() {
            log::debug!("Received signal {:?}", sig);

            if sig == SIGCHLD {
                if !self.check_alive() {
                    log::info!("no active programs left");
                    log::info!("stopping execution");
                    return;
                }
            } else {
                log::info!("terminating all programs");
                self.stop();
                log::info!("stopping execution");
                return;
            }
        }
    }

    fn check_alive(&mut self) -> bool {
        let mut idx = 0;
        while idx < self.programs.len() {
            let prog = &mut self.programs[idx];
            match prog.program.poll() {
                Some(_) => {
                    log::info!("{} died", prog.info);
                    self.programs.remove(idx);
                }
                None => {
                    idx += 1;
                }
            }
        }
        !self.programs.is_empty()
    }

    fn stop(&mut self) {
        log::debug!("sending all children the SIGTERM signal");

        while let Some(mut prog) = self.programs.pop() {
            prog.program.terminate().unwrap_or_else(|e| {
                log::warn!("failed to terminate {}: {:?}", prog.info, e);
            });

            match prog.program.wait_timeout(self.terminate_timeout) {
                Err(e) => log::warn!("failed to wait: {:?}", e),
                Ok(Some(_)) => {
                    log::info!("{} terminated", prog.info);
                }
                Ok(None) => {
                    log::warn!("timeout exceeded, killing {}", prog.info);
                    match prog.program.kill() {
                        Ok(_) => {
                            log::info!("{} killed", prog.info);
                        }
                        Err(e) => {
                            log::warn!("failed to kill {}: {:?}", prog.info, e);
                        }
                    }
                }
            }
        }
    }
}

impl Drop for Execution {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(PartialEq, Debug)]
pub struct ProgramInfo {
    pub name: String,
    pub pid: u32,
}

impl std::fmt::Display for ProgramInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}

pub struct Program {
    info: ProgramInfo,
    program: Popen,
}

impl Program {
    pub fn from_config(
        prog: &config::Program,
        output: &output::OutputFileFactory,
    ) -> Result<Program> {
        create_program(prog, &output).and_then(|popen| {
            let pid = popen
                .pid()
                .ok_or_else(|| string_error::new_err("could not obtain pid"))?;
            let prog = Program {
                info: ProgramInfo {
                    name: prog.name.clone(),
                    pid,
                },
                program: popen,
            };

            log::info!("{} started", prog.info);
            Ok(prog)
        })
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn info(&self) -> &ProgramInfo {
        &self.info
    }
}

fn create_program(cfg: &config::Program, output: &output::OutputFileFactory) -> Result<Popen> {
    assert!(!cfg.argv.is_empty());
    assert!(cfg.enabled);

    let env: Vec<(String, String)> = cfg
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let stdout = output.open(cfg.name.as_str())?;

    Exec::cmd(&cfg.argv[0])
        .args(&cfg.argv.as_slice()[1..])
        .env_extend(&env)
        .cwd(&cfg.cwd)
        .stdout(stdout)
        .stderr(Redirection::Merge)
        .popen()
        .map_err(|e| e.into())
}
