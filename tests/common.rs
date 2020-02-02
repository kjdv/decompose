use std::str::FromStr;
use std::io::BufRead;
use subprocess;
use std::path::PathBuf;

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
        let mut popen = subprocess::Exec::cmd(decompose_exe().as_os_str())
            .arg(data_file(config).as_os_str())
            .stdout(subprocess::Redirection::Pipe)
            .popen()
            .expect("popen");

        let (tx, rx) = std::sync::mpsc::channel();
        let mut f = std::io::BufReader::new(popen.stdout.take().unwrap());
        let handle = std::thread::spawn(move || {
            loop {
                let mut buffer = String::new();
                match f.read_line(&mut buffer) {
                    Ok(_) => tx.send(buffer).unwrap(),
                    Err(_) => return,
                };
            }
        });

        Fixture {
            process: popen,
            stdout: rx,
            thread: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.thread.take() {
            self.process.terminate().unwrap();
            self.process.wait().unwrap();
            h.join().unwrap();
        }
    }

    fn next_line(&self) -> String {
        let timeout = std::time::Duration::from_millis(100);
        self.stdout.recv_timeout(timeout).expect("timeout")
    }

    pub fn expect_line(&self, re: &str) -> Vec<String> { // returns captures, if any
        let re = regex::Regex::new(re).expect("valid regex");
        loop {
            let line = self.next_line();
            if let Some(_) = re.find(line.as_str()) {
                let mut result = Vec::<String>::new();
                let caps = re.captures(line.as_str()).unwrap();
                for idx in 1..re.captures_len() {
                    result.push(String::from_str(caps.get(idx).unwrap().as_str()).unwrap());
                }
                return result;
            };
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
    }
}
