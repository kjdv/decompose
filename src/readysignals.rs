extern crate nix;
extern crate regex;

use std::io;
use std::io::BufRead;
use std::ops::Add;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub trait ReadySignal {
    fn poll(&mut self) -> Result<bool>;
}

pub struct Nothing {}

impl Nothing {
    pub fn new() -> Nothing {
        Nothing {}
    }
}

impl ReadySignal for Nothing {
    fn poll(&mut self) -> Result<bool> {
        Ok(true)
    }
}

pub struct Manual<'a> {
    name: String,
    prompt: Option<Box<dyn FnOnce() -> io::Result<()> + 'a>>,
}

impl<'a> Manual<'a> {
    pub fn new(name: String) -> Manual<'a> {
        let prompt = Box::new(|| {
            let mut sink = String::new();
            io::stdin().read_line(&mut sink)?;
            Ok(())
        });
        Manual::new_with_prompt(name, prompt)
    }

    pub fn new_with_prompt(name: String, prompt: Box<dyn FnOnce() -> io::Result<()> + 'a>) -> Manual<'a> {
        Manual { name, prompt: Some(prompt) }
    }
}

impl ReadySignal for Manual<'_> {
    fn poll(&mut self) -> Result<bool> {
        if let Some(p) = self.prompt.take() {
            println!("Manually waiting for {}, press enter", self.name);
            p()?;
        }
        Ok(true)
    }
}

pub struct Timer<'a> {
    end: std::time::Instant,
    clock: Box<dyn FnMut() -> std::time::Instant + 'a>,
}

impl<'a> Timer<'a> {
    pub fn new(dur: std::time::Duration) -> Timer<'a> {
        let clock = Box::new(std::time::Instant::now);
        Timer::new_with_clock(dur, clock)
    }

    pub fn new_with_clock(
        dur: std::time::Duration,
        mut clock: Box<dyn FnMut() -> std::time::Instant + 'a>,
    ) -> Timer<'a> {
        let start = clock();
        Timer {
            end: start.add(dur),
            clock,
        }
    }
}

impl ReadySignal for Timer<'_> {
    fn poll(&mut self) -> Result<bool> {
        let now = (self.clock)();
        Ok(now >= self.end)
    }
}

pub struct Port {
    address: String,
}

impl Port {
    pub fn new(port: u16) -> Port {
        Port::new_with_host("127.0.0.1", port)
    }

    pub fn new_with_host(host: &str, port: u16) -> Port {
        Port {
            address: format!("{}:{}", host, port),
        }
    }
}

impl ReadySignal for Port {
    fn poll(&mut self) -> Result<bool> {
        Ok(std::net::TcpStream::connect(&self.address).is_ok())
    }
}

pub struct Stdout {
    regex: regex::Regex,
    reader: Option<std::io::BufReader<std::fs::File>>,
}

impl Stdout {
    pub fn new(filename: std::path::PathBuf, re: String) -> Result<Stdout> {
        let re = regex::Regex::new(re.as_str())?;
        let file = std::fs::File::open(filename)?;
        let reader = std::io::BufReader::new(file);

        Ok(Stdout {
            regex: re,
            reader: Some(reader),
        })
    }
}

impl ReadySignal for Stdout {
    fn poll(&mut self) -> Result<bool> {
        if let Some(mut reader) = self.reader.take() {
            let mut line = String::new();
            reader.read_line(&mut line)?;

            let rn: &[_] = &['\r', '\n'];
            let line = line.trim_end_matches(rn);

            if !self.regex.is_match(line) {
                self.reader.replace(reader);
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub struct Completed {
    pid: nix::unistd::Pid,
    ready: bool,
}

impl Completed {
    pub fn new(pid: u32) -> Completed {
        Completed {
            pid: nix::unistd::Pid::from_raw(pid as i32),
            ready: false,
        }
    }
}

impl ReadySignal for Completed {
    fn poll(&mut self) -> Result<bool> {
        use nix::sys::wait;

        if self.ready {
            return Ok(true);
        }

        let status = wait::waitpid(self.pid, None)?;
        match status {
            wait::WaitStatus::Exited(_, _) => {
                self.ready = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::ops::AddAssign;

    fn assert_is_true(rs: &mut dyn ReadySignal) {
        let status = rs.poll().expect("ok");
        assert_eq!(true, status);

        // somewhat check invariant: once a signal is ready it remains ready
        let status = rs.poll().expect("ok");
        assert_eq!(true, status);
    }

    fn assert_is_false(rs: &mut dyn ReadySignal) {
        let status = rs.poll().expect("ok");
        assert_eq!(false, status);
    }

    fn assert_is_err(rs: &mut dyn ReadySignal) {
        rs.poll().expect_err("err");
    }

    #[test]
    fn nothing() {
        let mut rs = Nothing::new();
        assert_is_true(&mut rs);
    }

    #[test]
    fn manual_ok() {
        let prompt = Box::new(|| Ok(()));

        let mut rs = Manual::new_with_prompt("test".to_string(), prompt);
        assert_is_true(&mut rs);
    }

    #[test]
    fn manual_err() {
        let prompt = Box::new(|| Err(io::Error::new(io::ErrorKind::InvalidInput, "blah")));

        let mut rs = Manual::new_with_prompt("test".to_string(), prompt);
        assert_is_err(&mut rs);
    }

    #[test]
    fn timer() {
        let epoch = std::time::Instant::now();
        let second = std::time::Duration::from_secs(1);
        let mut now = epoch;

        let clock = Box::new(|| {
            now.add_assign(second);
            now
        });

        let mut rs = Timer::new_with_clock(std::time::Duration::from_secs(3), clock);

        assert_is_false(&mut rs);
        assert_is_false(&mut rs);
        assert_is_true(&mut rs);
        assert_is_true(&mut rs);
    }

    #[test]
    fn port() {
        let mut rs = Port::new(9092);
        assert_is_false(&mut rs);

        // cheating on unit test rules: is opening a port okay?
        let _listener = std::net::TcpListener::bind("127.0.0.1:9092").expect("open 9292");

        assert_is_true(&mut rs);
    }

    #[test]
    fn stdout() {
        let tempdir = tempfile::Builder::new().tempdir().expect("tempir");
        let filename = "test.file";

        let mut buf = tempdir.path().to_path_buf();
        buf.push(filename);
        let mut f = std::fs::File::create(buf).expect("open for write");

        let mut buf = tempdir.path().to_path_buf();
        buf.push(filename);
        let mut rs = Stdout::new(buf, "^ready [0-9]+$".to_string()).expect("valid regex");

        assert_is_false(&mut rs);

        f.write_all(b"something\n").unwrap();
        f.flush().unwrap();

        assert_is_false(&mut rs);

        f.write_all(b"ready 123\n").unwrap();
        f.flush().unwrap();

        assert_is_true(&mut rs);

        f.write_all(b"more stuff\n").unwrap();
        f.flush().unwrap();

        assert_is_true(&mut rs);
    }

    #[test]
    fn completed() {
        let sink = std::fs::File::open("/dev/null").unwrap();
        let proc = subprocess::Exec::cmd("/bin/ls")
            .stdout(subprocess::Redirection::File(sink))
            .popen()
            .unwrap();

        let mut rs = Completed::new(proc.pid().unwrap());

        while !rs.poll().expect("poll") {}

        assert_is_true(&mut rs);
    }
}
