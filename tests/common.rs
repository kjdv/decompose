use std::sync::Once;
use std::str::FromStr;
use std::io::BufRead;
use subprocess;
use std::path::PathBuf;
use nix::sys::signal::{kill, SIGTERM};

static LOG_INIT: Once = Once::new();

fn bin_root() -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    path.pop();
    path
}

fn decompose_exe() -> PathBuf {
    let mut path = bin_root();
    path.push("decompose");
    path
}

fn data_file(name: &str) -> PathBuf {
    let mut path = PathBuf::from(file!());
    path.pop();
    path.push("data");
    path.push(name);
    path
}

pub struct Fixture {
    process: subprocess::Popen,
    reader: std::io::BufReader<std::fs::File>,
}

impl Fixture {
    pub fn new(config: &str) -> Fixture {
        LOG_INIT.call_once(|| {
            simple_logger::init_with_level(log::Level::Debug).expect("log init");
        });

        let mut popen = subprocess::Exec::cmd(decompose_exe().as_os_str())
            .arg("--debug")
            .arg(data_file(config).as_os_str())
            .stdout(subprocess::Redirection::Pipe)
            .popen()
            .expect("popen");

        let dur = std::time::Duration::from_millis(100);
        log::info!("waiting {:?} to let everything start", dur);
        std::thread::sleep(dur); // todo: find a better way to deal with these race conditions

        let reader = std::io::BufReader::new(popen.stdout.take().unwrap());
        Fixture {
            process: popen,
            reader: reader,
        }
    }

    pub fn stop(&mut self) {
        self.process.terminate().unwrap();
        self.process.wait().unwrap();
    }

    fn next_line(&mut self) -> String {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).expect("no input");
        assert_ne!(0, n);
        log::debug!("line: {}", line);
        line
    }

    fn expect_line(&mut self, re: &str) -> Vec<String> { // returns captures
        let re = regex::Regex::new(re).expect("valid regex");
        loop {
            let line = self.next_line();
            if let Some(_) = re.find(line.as_str()) {
                let caps = re.captures(line.as_str()).unwrap();
                let result: Vec<String> = caps.iter()
                    .map(|c| String::from_str(c.expect("match").as_str()).unwrap())
                    .collect();
                return result;
            }
        }
    }

    pub fn expect_start(&mut self) {
        self.expect_line(r"\[decompose::execution\] starting execution");
    }

    pub fn expect_stop(&mut self) {
        self.expect_line(r"\[decompose::execution\] stopping execution");
    }

    pub fn expect_program_starts(&mut self) -> ProgramInfo {
        let caps = self.expect_line(r"\[decompose::execution\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) started");
        ProgramInfo {
            name: caps.get(1).unwrap().to_string(),
            pid: caps.get(2).unwrap().to_string().parse().unwrap(),
        }
    }

    pub fn expect_program_dies(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} died", *prog);
        self.expect_line(re.as_str());
    }

    pub fn expect_program_terminates(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} terminated", *prog);
        self.expect_line(re.as_str());
    }

    pub fn expect_program_is_killed(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} killed", *prog);
        self.expect_line(re.as_str());
    }

    pub fn terminate_program(&self, program: &ProgramInfo) {
        kill(nix::unistd::Pid::from_raw(program.pid), SIGTERM).expect("term");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(PartialEq, Debug)]
pub struct ProgramInfo {
    pub name: String,
    pub pid: i32,
}

impl std::fmt::Display for ProgramInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}
