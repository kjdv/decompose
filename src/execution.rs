extern crate subprocess;

use log;
use std::error::Error;
use subprocess::{Popen, Exec};
use signal_hook::{iterator::Signals, SIGINT, SIGTERM, SIGCHLD};

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Execution {
    programs: Vec<(String, Popen)>
}

impl Execution {
    pub fn from_config(cfg: config::System) -> Result<Execution> {
        let mut progs = vec![];

        for p in &cfg.program {
            if p.enabled {
                log::info!("starting {:?}", p.name);

                match Execution::create_program(&p) {
                    Ok(popen) => progs.push((p.name.clone(), popen)),
                    Err(err) => return Err(err)
                }
            } else {
                log::info!("{:?} is disabled, skipping", p.name);
            }
        }

        Ok(Execution{programs: progs})
    }

    pub fn wait(&mut self) {
        log::debug!("waiting for SIGTERM");

        let signals = Signals::new(&[SIGINT, SIGTERM, SIGCHLD]).unwrap();

        for sig in signals.forever() {
            log::debug!("Received signal {:?}", sig);

            if sig == SIGCHLD {
                if !self.check_alive() {
                    log::info!("no active programs left");
                    return;
                }
            } else {
                log::info!("terminating all programs");
                self.stop();
                log::info!("done");
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
                    log::info!("{:?} exited with status {:?}", prog.0, status);
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
                Ok(Some(status)) => log::info!("{:?} exited with status {:?}", prog.0, status),
                Ok(None) => {
                    log::warn!("timeout exceeded, killing {:?}", prog.0);
                    prog.1.kill()
                        .unwrap_or_else(|e| {log::warn!("failed to kill: {:?}", e);});
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
