extern crate subprocess;

use log;
use std::error::Error;
use subprocess::{Popen, Exec};
use signal_hook::{iterator::Signals, SIGINT, SIGTERM, SIGCHLD};

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Execution<L: Listener> {
    programs: Vec<(String, Popen)>,
    listener: L,
}

impl<L: Listener> Execution<L> {
    pub fn from_config(cfg: config::System, listener: L) -> Result<Execution<L>> {
        let mut progs = vec![];

        listener.event(Event::Start());

        for p in &cfg.program {
            if p.enabled {

                match Execution::<L>::create_program(&p) {
                    Ok(popen) => {
                        listener.event(Event::ProgramStarted(&p.name, popen.pid().expect("pid")));
                        progs.push((p.name.clone(), popen))
                    },
                    Err(err) => return Err(err)
                }
            } else {
                log::info!("program {:?} is disabled, skipping", p.name);
            }
        }

        Ok(Execution{programs: progs, listener: listener})
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
            match prog.1.poll() {
                Some(status) => {
                    self.listener.event(Event::ProgramDied(&prog.0, status));
                    self.programs.remove(idx);
                }
                None => {idx += 1;},
            }
        }
        self.programs.len() > 0
    }

    fn stop(&mut self) {
        let timeout = std::time::Duration::new(1, 0);

        log::debug!("sending all children the SIGTERM signal");

        for prog in &mut self.programs {
            prog.1.terminate()
                .unwrap_or_else(|e| {
                    log::warn!("failed to terminate {:?}: {:?}", prog.0, e);
            });

            match prog.1.wait_timeout(timeout) {
                Err(e) => log::warn!("failed to wait: {:?}", e),
                Ok(Some(status)) => {
                    let e = Event::ProgramTerminated(&prog.0, status);
                    self.listener.event(e);
                },
                Ok(None) => {
                    log::warn!("timeout exceeded, killing {:?}", prog.0);
                    match prog.1.kill() {
                        Ok(_) => {
                            let e = Event::ProgramKilled(&prog.0, prog.1.exit_status().expect("exit status"));
                            self.listener.event(e);
                        },
                        Err(e) => {log::warn!("failed to kill: {:?}", e);}
                    }
                }
            }
        }
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

pub enum Event<'a> {
    Start(),
    Stop(),
    ProgramStarted(&'a String, u32),
    ProgramDied(&'a String, subprocess::ExitStatus),
    ProgramTerminated(&'a String, subprocess::ExitStatus),
    ProgramKilled(&'a String, subprocess::ExitStatus),
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
            Event::ProgramStarted(name, pid) => log::info!("program {} at pid {} started", name, pid),
            Event::ProgramDied(name, status) =>
                log::info!("program {} at died, status {:?}", name, status),
            Event::ProgramTerminated(name, status) =>
                log::info!("program {} at terminated, status {:?}", name, status),
            Event::ProgramKilled(name, status) =>
                log::info!("program {} killed, status {:?}", name, status),
        }
    }
}
