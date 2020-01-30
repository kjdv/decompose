extern crate subprocess;

use log;
use std::error::Error;
use subprocess::{Popen, Exec};
use signal_hook::{iterator::Signals, SIGINT, SIGTERM, SIGCHLD};

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Execution {
    programs: Vec<Popen>
}

impl Execution {
    pub fn from_config(cfg: config::System) -> Result<Execution> {
        let mut progs = vec![];

        for p in &cfg.program {
            if p.enabled {
                log::info!("starting {:?}", p.name);

                match Execution::create_program(&p) {
                    Ok(popen) => progs.push(popen),
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
                if !self.alive() {
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

    fn alive(&mut self) -> bool {
        for prog in &mut self.programs {
            if prog.poll().is_none() {
                return true;
            }
        }
        false
    }

    fn stop(&mut self) {
        let timeout = std::time::Duration::new(1, 0);

        log::debug!("sending all children the SIGTERM signal");

        for prog in &mut self.programs {
            prog.terminate()
                .unwrap_or_else(|e| {
                    log::warn!("failed to terminate: {:?}", e);
            });

            match prog.wait_timeout(timeout) {
                Err(e) => log::warn!("failed to wait: {:?}", e),
                Ok(Some(_)) => (),
                Ok(None) => {
                    log::info!("timeout exceeded, killing");
                    prog.kill()
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
