use std::sync::Once;
use std::str::FromStr;
use std::io::BufRead;
use subprocess;
use std::path::PathBuf;

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
    pub process: subprocess::Popen,

    stdout: std::sync::mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Fixture {
    pub fn new(config: &str) -> Fixture {
        LOG_INIT.call_once(|| {
            simple_logger::init_with_level(log::Level::Info).expect("log init");
        });

        let mut popen = subprocess::Exec::cmd(decompose_exe().as_os_str())
            .arg("--debug")
            .arg(data_file(config).as_os_str())
            .stdout(subprocess::Redirection::Pipe)
            .popen()
            .expect("popen");

        let (tx, rx) = std::sync::mpsc::channel();
        let mut f = std::io::BufReader::new(popen.stdout.take().unwrap());
        let handle = std::thread::spawn(move || {
            loop {
                let mut buffer = String::new();
                if let Ok(n) = f.read_line(&mut buffer) {
                    if n > 0 {
                        log::debug!("received from program: {}", buffer);
                        tx.send(buffer).expect("send");
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        });

        Fixture {
            process: popen,
            stdout: rx,
            thread: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.process.terminate().unwrap();
        self.process.wait().unwrap();
        if let Some(h) = self.thread.take() {
            h.join().unwrap();
        }
    }

    fn next_line(&self) -> String {
        let timeout = std::time::Duration::from_millis(100);
        let line = self.stdout.recv_timeout(timeout).expect("timeout");
        log::debug!("got line {}", line);
        line
    }

    fn expect_line(&self, re: &str) -> Vec<String> { // returns captures
        let re = regex::Regex::new(re).expect("valid regex");
        loop {
            let line = self.next_line();
            if let Some(_) = re.find(line.as_str()) {
                let caps = re.captures(line.as_str()).unwrap();
                let result: Vec<String> = caps.iter()
                    .map(|c| String::from_str(c.expect("match").as_str()).unwrap())
                    .collect();
                return result;
            } else {
                log::info!("discarding {}", line);
            }
        }
    }

    pub fn expect_start(&self) {
        self.expect_line(r"\[decompose::execution\] starting execution");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}
