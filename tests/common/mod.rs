use nix::sys::signal::{kill, SIGTERM};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Once;
use subprocess;

static LOG_INIT: Once = Once::new();
static BIN_INIT: Once = Once::new();

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
    path.pop();
    path.push("data");
    path.push(name);
    path
}

fn testrun_dir() -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(root);
    path.push("target");
    path.push("testrun");
    path
}

fn helper_path(name: &str) -> PathBuf {
    let mut path = bin_root();
    path.push("examples");
    path.push(name);
    path
}

fn link_helpers() {
    const HELPERS: [&str; 3] = ["sigterm_intercept", "http_server", "http_proxy"];

    let mut target_dir = testrun_dir();
    target_dir.push("bin");
    std::fs::create_dir_all(target_dir).expect("mkdir");

    for helper in &HELPERS {
        let source = helper_path(helper);
        let mut target = testrun_dir();
        target.push("bin");
        target.push(helper);
        let _ = std::fs::remove_file(&target);
        std::os::unix::fs::symlink(source, target).expect("symlink");
    }
}

pub struct Fixture {
    process: subprocess::Popen,
    reader: std::io::BufReader<std::fs::File>,
}

#[allow(dead_code)]
impl Fixture {
    pub fn new(config: &str) -> Fixture {
        LOG_INIT.call_once(|| {
            simple_logger::init_with_level(log::Level::Info).expect("log init");
        });
        BIN_INIT.call_once(link_helpers);

        let mut popen = subprocess::Exec::cmd(decompose_exe().as_os_str())
            .arg("--debug")
            .arg(data_file(config).as_os_str())
            .stdout(subprocess::Redirection::Pipe)
            .stdin(subprocess::Redirection::Pipe)
            .popen()
            .expect("popen");

        let reader = std::io::BufReader::new(popen.stdout.take().unwrap());
        Fixture {
            process: popen,
            reader,
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
        print!("{}", line);
        line
    }

    pub fn expect_line(&mut self, re: &str) -> Vec<String> {
        // returns captures
        let re = regex::Regex::new(re).expect("valid regex");
        loop {
            let line = self.next_line();
            if re.find(line.as_str()).is_some() {
                log::debug!("match: {}", line);

                let caps = re.captures(line.as_str()).unwrap();
                let result: Vec<String> = caps
                    .iter()
                    .map(|c| String::from_str(c.expect("match").as_str()).unwrap())
                    .collect();
                return result;
            } else {
                log::debug!("discard: {}", line);
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
        let caps =
            self.expect_line(r"\[decompose::execution\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) started");
        ProgramInfo {
            name: caps.get(1).unwrap().to_string(),
            pid: caps.get(2).unwrap().to_string().parse().unwrap(),
        }
    }

    pub fn expect_program_ready(&mut self) -> ProgramInfo {
        let caps =
            self.expect_line(r"\[decompose::execution\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) ready");
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

    pub fn send_stdin(&mut self, data: &str) {
        self.process.stdin.as_ref().unwrap().write_all(data.as_bytes()).unwrap();
        self.process.stdin.as_ref().unwrap().flush().unwrap();
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

#[allow(dead_code)]
pub fn http_get(port: u16, path: &str) -> (u16, String) {
    let url = format!("http://127.0.0.1:{}/{}", port, path);
    let res = reqwest::blocking::get(&url).expect("http get");
    let status = res.status().as_u16();
    let body = res.text().unwrap();

    (status, body)
}
