extern crate futures;
extern crate nix;
extern crate regex;
extern crate tokio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Result = std::result::Result<bool, tokio::io::Error>;

pub async fn nothing() -> Result {
    Ok(true)
}

pub async fn manual(name: &str) -> Result {
    let mut stdout = tokio::io::stdout();
    stdout
        .write(format!("Manually waiting for {}, press enter", name).as_bytes())
        .await?;
    stdout.flush().await?;

    let mut stdin = tokio::io::stdin();
    let mut buf = [0; 1];
    stdin.read(&mut buf).await?;
    Ok(true)
}

/*
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
            wait::WaitStatus::Exited(_, 0) => {
                self.ready = true;
                Ok(true)
            }
            wait::WaitStatus::Exited(_, n) => {
                self.ready = true;
                let e = format!("non-zero exit status {}", n);
                Err(string_error::new_err(e.as_str()))
            }
            _ => Ok(false),
        }
    }
}

*/

#[cfg(test)]
mod tests {
    extern crate tokio;

    use super::*;

    #[tokio::test]
    async fn test_nothing() {
        let result = nothing().await.expect("nothing");
        assert!(result);
    }

    /*
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
        let proc = subprocess::Exec::cmd("/bin/ls")
            .stdout(subprocess::NullFile)
            .popen()
            .expect("/bin/ls");

        let mut rs = Completed::new(proc.pid().unwrap());

        while !rs.poll().expect("poll") {}

        assert_is_true(&mut rs);
    }

    #[test]
    fn completed_failing_process() {
        let sink = std::fs::File::open("/dev/null").unwrap();
        let proc = subprocess::Exec::cmd("/bin/ls")
            .arg("no such file or directory")
            .stdout(subprocess::Redirection::File(sink))
            .popen()
            .unwrap();

        let mut rs = Completed::new(proc.pid().unwrap());

        while let Ok(s) = rs.poll() {
            assert_eq!(false, s);
        }
        // success, error returned
    }

    */
}
