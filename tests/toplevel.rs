use nix::sys::signal::{kill, SIGTERM};
use std::io::BufRead;
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

struct Fixture {
    process: subprocess::Popen,
    reader: std::io::BufReader<std::fs::File>,
}

impl Fixture {
    fn new(config: &str) -> Fixture {
        LOG_INIT.call_once(|| {
            simple_logger::init_with_level(log::Level::Info).expect("log init");
        });
        BIN_INIT.call_once(link_helpers);

        let mut popen = subprocess::Exec::cmd(decompose_exe().as_os_str())
            .arg("--debug")
            .arg(data_file(config).as_os_str())
            .stdout(subprocess::Redirection::Pipe)
            .popen()
            .expect("popen");

        let reader = std::io::BufReader::new(popen.stdout.take().unwrap());
        Fixture {
            process: popen,
            reader,
        }
    }

    fn stop(&mut self) {
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

    fn expect_line(&mut self, re: &str) -> Vec<String> {
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

    fn expect_start(&mut self) {
        let dur = std::time::Duration::from_millis(100);
        log::info!("waiting {:?} to let everything start", dur);
        std::thread::sleep(dur); // todo: find a better way to deal with these race conditions

        self.expect_line(r"\[decompose::execution\] starting execution");
    }

    fn expect_stop(&mut self) {
        self.expect_line(r"\[decompose::execution\] stopping execution");
    }

    fn expect_program_starts(&mut self) -> ProgramInfo {
        let caps =
            self.expect_line(r"\[decompose::execution\] ([a-zA-Z][a-zA-Z0-9]+):([0-9]+) started");
        ProgramInfo {
            name: caps.get(1).unwrap().to_string(),
            pid: caps.get(2).unwrap().to_string().parse().unwrap(),
        }
    }

    fn expect_program_dies(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} died", *prog);
        self.expect_line(re.as_str());
    }

    fn expect_program_terminates(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} terminated", *prog);
        self.expect_line(re.as_str());
    }

    fn expect_program_is_killed(&mut self, prog: &ProgramInfo) {
        let re = format!("\\[decompose::execution\\] {} killed", *prog);
        self.expect_line(re.as_str());
    }

    fn terminate_program(&self, program: &ProgramInfo) {
        kill(nix::unistd::Pid::from_raw(program.pid), SIGTERM).expect("term");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(PartialEq, Debug)]
struct ProgramInfo {
    name: String,
    pid: i32,
}

impl std::fmt::Display for ProgramInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    let url = format!("http://127.0.0.1:{}/{}", port, path);
    let res = reqwest::blocking::get(&url).expect("http get");
    let status = res.status().as_u16();
    let body = res.text().unwrap();

    (status, body)
}

////
//
// start / stop
//
////

#[test]
fn can_start_and_stop() {
    let mut f = Fixture::new("single.toml");
    f.expect_start();
    let prog = f.expect_program_starts();
    f.stop();
    f.expect_program_terminates(&prog);
    f.expect_stop();
}

#[test]
fn stop_if_all_programs_dies() {
    let mut f = Fixture::new("single.toml");
    f.expect_start();

    let prog = f.expect_program_starts();
    f.terminate_program(&prog);
    f.expect_program_dies(&prog);
    f.expect_stop();
}

#[test]
fn program_is_killed_if_it_catches_sigterm() {
    let mut f = Fixture::new("diehard.toml");
    f.expect_start();

    let prog = f.expect_program_starts();
    f.stop();
    f.expect_program_is_killed(&prog);
    f.expect_stop();
}

////
//
// ensemble
//
////

#[test]
fn starts_and_stops_in_the_right_order() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let srv = f.expect_program_starts();
    assert_eq!("server", srv.name);

    let proxy = f.expect_program_starts();
    assert_eq!("proxy", proxy.name);

    let (status, body) = http_get(9091, "hello");
    assert_eq!(200, status);
    assert_eq!("hello!\n", body);

    f.stop();

    f.expect_program_terminates(&proxy);
    f.expect_program_terminates(&srv);
    f.expect_stop();
}

#[test]
fn sets_args() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "args?idx=1");
    assert_eq!(200, status);
    assert_eq!("extra", body);

    f.stop();
    f.expect_stop();
}

#[test]
fn sets_env() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "env?key=FOO");
    assert_eq!(200, status);
    assert_eq!("BAR", body);
}

#[test]
fn sets_cwd() {
    let mut f = Fixture::new("ensemble.toml");
    f.expect_start();

    let (status, body) = http_get(9090, "cwd");
    assert_eq!(200, status);
    assert!(body.ends_with("target/testrun"));
}
