extern crate regex;
extern crate nix;

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
        Nothing{}
    }
}

impl ReadySignal for Nothing {
    fn poll(&mut self) -> Result<bool> {
        Ok(true)
    }
}

pub struct Manual {
    name: String,
    prompt: fn() -> io::Result<()>,
}

impl Manual {
    pub fn new(name: String) -> Manual {
        Manual::new_with_prompt(name, Manual::stdin)
    }

    pub fn new_with_prompt(name: String, prompt: fn() -> io::Result<()>) -> Manual {
        Manual { name, prompt }
    }

    fn stdin() -> io::Result<()> {
        let mut sink = String::new();
        io::stdin().read_line(&mut sink)?;
        Ok(())
    }
}

impl ReadySignal for Manual {
    fn poll(&mut self) -> Result<bool> {
        println!("Manually waiting for {}, press enter", self.name);
        (self.prompt)()?;
        Ok(true)
    }
}

pub struct Timer<'a> {
    end: std::time::SystemTime,
    clock: Box<dyn FnMut() -> std::time::SystemTime + 'a>,
}

impl<'a> Timer<'a> {
    pub fn new(dur: std::time::Duration) -> Timer<'a> {
        let clock = Box::new(std::time::SystemTime::now);
        Timer::new_with_clock(dur, clock)
    }

    pub fn new_with_clock(
        dur: std::time::Duration,
        mut clock: Box<dyn FnMut() -> std::time::SystemTime + 'a>,
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
    filename: std::path::PathBuf,
    regex: regex::Regex,
}

impl Stdout {
    pub fn new(filename: std::path::PathBuf, re: String) -> Result<Stdout> {
        let re = regex::Regex::new(re.as_str())?;
        Ok(Stdout {
            filename,
            regex: re,
        })
    }
}

impl ReadySignal for Stdout {
    fn poll(&mut self) -> Result<bool> {
        // there are smarter and faster ways to do this, but simplest is to just grep the whole
        // file on each poll

        let filename = self.filename.to_owned();
        let file = std::fs::File::open(filename)?;
        let reader = std::io::BufReader::new(file);

        let m = reader.lines().any(|line| {
            println!("{:?}", line);
            match line {
                Ok(line) => self.regex.is_match(line.as_str()),
                Err(_) => false,
            }
        });

        Ok(m)
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
            },
            _ => Ok(false)
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
        fn prompt() -> io::Result<()> {
            Ok(())
        }

        let mut rs = Manual::new_with_prompt("test".to_string(), prompt);
        assert_is_true(&mut rs);
    }

    #[test]
    fn manual_err() {
        fn prompt() -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "blah"))
        }

        let mut rs = Manual::new_with_prompt("test".to_string(), prompt);
        assert_is_err(&mut rs);
    }

    #[test]
    fn timer() {
        let epoch = std::time::SystemTime::now();
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

        let mut rs = Stdout::new(buf, "^ready [0-9]+$".to_string()).expect("valid regex");

        assert_is_err(&mut rs); // file does not (yet? exist)

        let mut buf = tempdir.path().to_path_buf();
        buf.push(filename);
        let mut f = std::fs::File::create(buf).expect("open for read");

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
        let proc = subprocess::Exec::cmd("/bin/ls")
            .popen()
            .unwrap();

        let mut rs = Completed::new(proc.pid().unwrap());

        while !rs.poll().expect("poll") {}

        assert_is_true(&mut rs);
    }
}
