extern crate escargot;

use nix::sys::signal::{kill, SIGTERM};
use std::convert::TryInto;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::str::FromStr;
use std::sync::Once;

static LOG_INIT: Once = Once::new();
static BIN_INIT: Once = Once::new();

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn bin_root() -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    path.pop();
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
    const HELPERS: [&str; 3] = ["sigterm_intercept", "server", "proxy"];

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

fn terminate(proc: &mut Child, timeout: f64) -> bool {
    use std::time::{Duration, Instant};

    let end = Instant::now() + Duration::from_secs_f64(timeout);

    let pid: i32 = proc.id().try_into().unwrap();
    let _ = kill(nix::unistd::Pid::from_raw(pid), SIGTERM);

    loop {
        if proc.try_wait().expect("wait").is_some() {
            return true;
        }

        if Instant::now() > end {
            return false;
        }
    }
}

pub struct Fixture {
    process: Option<Child>,
    reader: std::io::BufReader<std::process::ChildStdout>,
    writer: std::io::BufWriter<std::process::ChildStdin>,
}

#[allow(dead_code)]
impl Fixture {
    pub fn new(config: &str) -> Fixture {
        LOG_INIT.call_once(|| {
            simple_logger::init_with_level(log::Level::Info).expect("log init");
        });
        BIN_INIT.call_once(link_helpers);

        let mut proc = escargot::CargoBuild::new()
            .run()
            .expect("cargo run")
            .command()
            .arg("--debug")
            .arg(data_file(config))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start");

        let reader = std::io::BufReader::new(proc.stdout.take().unwrap());
        let writer = std::io::BufWriter::new(proc.stdin.take().unwrap());
        Fixture {
            process: Some(proc),
            reader,
            writer,
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut proc) = self.process.take() {
            if !terminate(&mut proc, 0.1) {
                proc.kill().unwrap();
                proc.wait().unwrap();
            }
        }
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
        self.expect_line(r"\[decompose::executor\] starting execution");
    }

    pub fn expect_stop(&mut self) {
        self.expect_line(r"\[decompose::executor\] stopping execution");
    }

    pub fn expect_program_starts(&mut self) -> ProgramInfo {
        let caps =
            self.expect_line(r"\[decompose::executor\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) started");
        ProgramInfo {
            name: caps.get(1).unwrap().to_string(),
            pid: caps.get(2).unwrap().to_string().parse().unwrap(),
        }
    }

    pub fn expect_program_ready(&mut self) -> ProgramInfo {
        let caps =
            self.expect_line(r"\[decompose::executor\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) ready");
        ProgramInfo {
            name: caps.get(1).unwrap().to_string(),
            pid: caps.get(2).unwrap().to_string().parse().unwrap(),
        }
    }

    pub fn expect_program_dies(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::executor\\] {} stopped", *prog);
        self.expect_line(re.as_str());
    }

    pub fn expect_program_terminates(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::executor\\] {} terminated", *prog);
        self.expect_line(re.as_str());
    }

    pub fn expect_program_is_killed(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::executor\\] {} killed", *prog);
        self.expect_line(re.as_str());
    }

    pub fn terminate_program(&self, program: &ProgramInfo) {
        kill(nix::unistd::Pid::from_raw(program.pid), SIGTERM).expect("term");
    }

    pub fn send_stdin(&mut self, data: &str) {
        self.writer.write_all(data.as_bytes()).expect("write");
        self.writer.flush().unwrap();
    }

    pub fn expect_exited(&mut self) {
        use std::time::{Duration, Instant};

        if self.process.is_none() {
            return;
        }

        let end = Instant::now() + Duration::from_secs(1);

        while self
            .process
            .as_mut()
            .unwrap()
            .try_wait()
            .expect("wait")
            .is_none()
        {
            assert!(Instant::now() <= end);
        }
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
pub fn call(port: u16, path: &str) -> Result<String> {
    let address = format!("127.0.0.1:{}", port);
    let mut stream = std::net::TcpStream::connect(address.as_str())?;

    stream.write_all(path.as_bytes())?;
    stream.flush()?;

    let mut buf = [0; 512];
    let size = stream.read(&mut buf)?;

    let response = String::from_utf8_lossy(&buf[0..size]);
    Ok(response.to_string())
}
