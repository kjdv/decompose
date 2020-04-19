extern crate string_error;
extern crate subprocess;

use log;
use signal_hook::{iterator::Signals, SIGCHLD, SIGINT, SIGTERM};
use std::error::Error;
use subprocess::{Exec, Popen, Redirection};
use std::ops::Add;

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
        let start = std::time::SystemTime::now();

        while !list.poll()? {
            log::debug!("not ready yet");
            std::thread::sleep(sleep_time);

            if let Some(dur) = cfg.start_timeout {
                let end = start.add(std::time::Duration::from_secs_f64(dur));

                if std::time::SystemTime::now() >= end {
                    log::error!("timed out waiting to start");
                    return Err(string_error::new_err("timed out waiting to start"));
                }
            }
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
    ready: Box<dyn readysignals::ReadySignal>,
}

impl Program {
    pub fn from_config(
        prog: &config::Program,
        output: &output::OutputFileFactory,
    ) -> Result<Program> {
        create_program(prog, &output).and_then(|(popen, rs)| {
            let pid = popen
                .pid()
                .ok_or_else(|| string_error::new_err("could not obtain pid"))?;
            let prog = Program {
                info: ProgramInfo {
                    name: prog.name.clone(),
                    pid,
                },
                program: popen,
                ready: rs,
            };

            log::info!("{} started", prog.info);
            Ok(prog)
        })
    }

    pub fn is_ready(&mut self) -> Result<bool> {
        let r = self.ready.poll();
        match r {
            Ok(true) => log::info!("{} ready", self.info),
            Ok(false) => log::debug!("{} not yet ready", self.info),
            Err(_) => (),
        }
        r
    }

    pub fn info(&self) -> &ProgramInfo {
        &self.info
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.program.terminate();
        let _ = self.program.kill();
        let _ = self.program.wait();
    }
}

fn create_program(cfg: &config::Program, output: &output::OutputFileFactory) -> Result<(Popen, Box<dyn readysignals::ReadySignal>)> {
    assert!(!cfg.argv.is_empty());
    assert!(cfg.enabled);

    let env: Vec<(String, String)> = cfg
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let (stdout, path) = output.open(cfg.name.as_str())?;

    let proc = Exec::cmd(&cfg.argv[0])
        .args(&cfg.argv.as_slice()[1..])
        .env_extend(&env)
        .cwd(&cfg.cwd)
        .stdout(stdout)
        .stderr(Redirection::Merge)
        .popen()?;

    let rs: Box<dyn readysignals::ReadySignal> = match &cfg.ready {
        config::ReadySignal::Nothing => {
            Box::new(readysignals::Nothing::new())
        },
        config::ReadySignal::Manual => {
            Box::new(readysignals::Manual::new(cfg.name.clone()))
        },
        config::ReadySignal::Timer(s) => {
            Box::new(readysignals::Timer::new(std::time::Duration::from_secs_f64(*s)))
        },
        config::ReadySignal::Completed => {
            //Box::new(readysignals::Nothing::new())
            let pid = proc.pid().ok_or_else(|| string_error::new_err("no pid"))?;
            Box::new(readysignals::Completed::new(pid))
        }
        config::ReadySignal::Port(p) => {
            Box::new(readysignals::Port::new(*p))
        },
        config::ReadySignal::Stdout(re) => {
            let r = readysignals::Stdout::new(path, re.to_string())?;
            Box::new(r)
        }
    };

    Ok((proc, rs))
}
