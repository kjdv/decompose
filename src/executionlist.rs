extern crate string_error;

use std::collections::HashSet;
use log;

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct ExecutionList {
    running: Vec<execution::Program>,
    pending: Vec<config::Program>,
    ready: HashSet<String>,

    output_factory: output::OutputFileFactory,
}

impl ExecutionList {
    pub fn from_system(cfg: &config::System, output_factory: output::OutputFileFactory) -> ExecutionList {
        ExecutionList{
            running: Vec::new(),
            pending: cfg.program.clone(),
            ready: HashSet::new(),
            output_factory,
        }
    }

    pub fn poll(&mut self) -> Result<bool> {
        // returns true if no more polling needs to be done

        for prog in &self.running {
            if !self.ready.contains(&prog.info().name) && prog.is_ready() {
                log::info!("{:?} ready", prog.info().name);
                self.ready.insert(prog.info().name.clone());
            }
        }

        for (pos, prog) in self.pending.iter().enumerate() {
            if !prog.enabled {
                log::info!("{:?} disabled, skipping", prog.name);
                self.pending.remove(pos);
                break;
            }
            if self.is_startable(prog) {
                log::info!("{:?} starting", prog.name);
                let eprog = execution::Program::from_config(prog, &self.output_factory)?;
                self.running.push(eprog);
                self.pending.remove(pos);
                break;
            }
        }

        Ok(self.pending.is_empty() && self.running.len() == self.ready.len())
    }

    pub fn reap(&mut self) -> Vec<execution::Program> {
        self.running.drain(0..).collect()
    }

    fn is_startable(&self, prog: &config::Program) -> bool {
        prog.depends.iter().all(|d| self.ready.contains(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
