extern crate string_error;

use log;
use std::collections::HashSet;

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct ExecutionList {
    running: Vec<execution::Program>,
    pending: Vec<config::Program>,
    ready: HashSet<String>,

    output_factory: output::OutputFileFactory,
}

impl ExecutionList {
    pub fn from_system(
        cfg: &config::System,
        output_factory: output::OutputFileFactory,
    ) -> ExecutionList {
        ExecutionList {
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
    extern crate tempfile;
    use tempfile::Builder;

    fn root() -> tempfile::TempDir {
        Builder::new().tempdir().unwrap()
    }

    fn run(toml: &str) -> Vec<execution::Program> {
        let sys = config::System::from_toml(toml).unwrap();

        let r = root();
        let output = output::OutputFileFactory::new(&r.path().to_str().unwrap()).unwrap();
        let mut exelist = ExecutionList::from_system(&sys, output);

        loop {
            if exelist.poll().unwrap() {
                return exelist.reap();
            }
        }
    }

    #[test]
    fn starts_in_dependency_order() {
        let toml = r#"
        [[program]]
        name = "last"
        argv = ["/bin/ls"]
        depends = ["middle"]

        [[program]]
        name = "first"
        argv = ["/bin/ls"]

        [[program]]
        name = "middle"
        argv = ["/bin/ls"]
        depends = ["first"]
        "#;

        let progs = run(toml);

        assert_eq!("first", progs[0].info().name);
        assert_eq!("middle", progs[1].info().name);
        assert_eq!("last", progs[2].info().name);
    }

    #[test]
    fn mutlitple_depends() {
        let toml = r#"
        [[program]]
        name = "last"
        argv = ["/bin/ls"]
        depends = ["one", "two"]

        [[program]]
        name = "one"
        argv = ["/bin/ls"]

        [[program]]
        name = "two"
        argv = ["/bin/ls"]
        "#;

        let progs = run(toml);

        assert_eq!("one", progs[0].info().name);
        assert_eq!("two", progs[1].info().name);
        assert_eq!("last", progs[2].info().name);
    }
}
