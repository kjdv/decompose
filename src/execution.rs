extern crate subprocess;
extern crate string_error;

use log;
use std::error::Error;
use subprocess::{Popen, Exec};
use signal_hook::{iterator::Signals, SIGINT, SIGTERM, SIGCHLD};

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Execution<L: Listener> {
    programs: Vec<Program>,
    listener: L,
    terminate_timeout: std::time::Duration,
}

impl<L: Listener> Execution<L> {
    pub fn from_config(cfg: config::System, listener: L) -> Result<Execution<L>> {
        listener.event(Event::Start());
        let mut execution = Execution{
            programs: Vec::new(),
            listener: listener,
            terminate_timeout: std::time::Duration::from_secs_f64(cfg.terminate_timeout),
        };

        for p in &cfg.program {
            if p.enabled {

                match Execution::<L>::create_program(&p) {
                    Ok(popen) => {
                        let pid = popen.pid()
                            .ok_or(string_error::new_err("could not obtain pid"))?;
                        let prog = Program{
                            info: ProgramInfo {
                                name: p.name.clone(),
                                pid: pid,
                            },
                            popen: popen,
                        };

                        let e = Event::ProgramStarted(&prog.info);
                        execution.listener.event(e);

                        execution.programs.push(prog)
                    },
                    Err(err) => return Err(err)
                }
            } else {
                log::info!("program {:?} is disabled, skipping", p.name);
            }
        }

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
                    self.listener.event(Event::Stop());
                    return;
                }
            } else {
                log::info!("terminating all programs");
                self.stop();
                log::info!("done");
                self.listener.event(Event::Stop());
                return;
            }
        }

    }

    fn check_alive(&mut self) -> bool {
        let mut idx = 0;
        while idx < self.programs.len() {
            let prog = &mut self.programs[idx];
            match prog.popen.poll() {
                Some(status) => {
                    log::debug!("{} died, stauts={:?}", prog.info, status);
                    self.listener.event(Event::ProgramDied(&prog.info));
                    self.programs.remove(idx);
                }
                None => {idx += 1;},
            }
        }
        self.programs.len() > 0
    }

    fn stop(&mut self) {
        log::debug!("sending all children the SIGTERM signal");

        for prog in &mut self.programs {
            prog.popen.terminate()
                .unwrap_or_else(|e| {
                    log::warn!("failed to terminate {}: {:?}", prog.info, e);
            });

            match prog.popen.wait_timeout(self.terminate_timeout) {
                Err(e) => log::warn!("failed to wait: {:?}", e),
                Ok(Some(status)) => {
                    log::debug!("terminated {}, status={:?}", prog.info, status);
                    let e = Event::ProgramTerminated(&prog.info);
                    self.listener.event(e);
                },
                Ok(None) => {
                    log::warn!("timeout exceeded, killing {}", prog.info);
                    match prog.popen.kill() {
                        Ok(_) => {
                            log::debug!("killed {}", prog.info);
                            let e = Event::ProgramKilled(&prog.info);
                            self.listener.event(e);
                        },
                        Err(e) => {log::warn!("failed to kill {}: {:?}", prog.info, e);}
                    }
                }
            }
        }

        self.programs.clear();
    }

    fn create_program(cfg: &config::Program) -> Result<Popen> {
        assert!(cfg.argv.len() > 0);
        assert!(cfg.enabled);

        let env: Vec<(String, String)>= cfg.env.iter()
            .map(|(k, v)| (k.clone(),v.clone()))
            .collect();

        Exec::cmd(&cfg.argv[0])
            .args(&cfg.argv.as_slice()[1..])
            .env_extend(&env)
            .cwd(&cfg.cwd)
            .popen()
            .map_err(|e| e.into())
    }
}

impl<L: Listener> Drop for Execution<L> {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct ProgramInfo {
    pub name: String,
    pub pid: u32,
}

struct Program {
    info: ProgramInfo,
    popen: Popen,
}

impl std::fmt::Display for ProgramInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}

pub enum Event<'a> {
    Start(),
    Stop(),
    ProgramStarted(&'a ProgramInfo),
    ProgramDied(&'a ProgramInfo),
    ProgramTerminated(&'a ProgramInfo),
    ProgramKilled(&'a ProgramInfo),
}

pub trait Listener {
    fn event(&self, e: Event);
}

pub type EventLogger = ();

impl Listener for EventLogger {
    fn event(&self, e: Event) {
        match e {
            Event::Start() => log::info!("starting execution"),
            Event::Stop() => log::info!("stopping execution"),
            Event::ProgramStarted(info) => log::info!("{} started", info),
            Event::ProgramDied(info) =>
                log::info!("{} died", info),
            Event::ProgramTerminated(info) =>
                log::info!("{} terminated", info),
            Event::ProgramKilled(info) =>
                log::info!("{} killed", info),
        }
    }
}
