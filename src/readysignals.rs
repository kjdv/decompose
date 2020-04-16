extern crate regex;

use std::io;
use std::ops::Add;
use std::io::BufRead;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub trait ReadySignal {
    fn poll(&mut self) -> Result<bool>;
}

pub type Nothing = ();

impl ReadySignal for Nothing {
    fn poll(&mut self) -> Result<bool> {
        Ok(true)
    }
}

pub struct Manual {
    name: String
}

impl Manual {
    pub fn new(name: String) -> Manual {
        Manual{
            name
        }
    }
}

impl ReadySignal for Manual {
    fn poll(&mut self) -> Result<bool> {
        println!("Manually waiting for {}, press enter", self.name);
        let mut sink = String::new();
        io::stdin().read_line(&mut sink)?;
        Ok(true)
    }
}

pub struct Timer {
    end: std::time::SystemTime,
}

impl Timer {
    pub fn new(dur: std::time::Duration) -> Timer {
        let start = std::time::SystemTime::now();
        Timer {
            end: start.add(dur)
        }
    }
}

impl ReadySignal for Timer {
    fn poll(&mut self) -> Result<bool> {
        let now = std::time::SystemTime::now();
        Ok(now >= self.end)
    }
}

pub struct Port {
    address: String,
}

impl Port {
    pub fn new(host: &str, port: u16) -> Port {
        Port{
            address: format!("{}:{}", host, port)
        }
    }

    pub fn new_local(port: u16) -> Port {
        Port::new("127.0.0.1", port)
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
            match line {
                Ok(line) => self.regex.is_match(line.as_str()),
                Err(_) => false,
            }
        });

        Ok(m)
    }
}

pub struct Completed<'a> {
    proc: &'a mut subprocess::Popen,
}

impl<'a> Completed<'a> {
    pub fn new(proc: &'a mut subprocess::Popen) -> Completed<'a> {
        Completed {
            proc
        }
    }
}

impl<'a> ReadySignal for Completed<'a> {
    fn poll(&mut self) -> Result<bool> {
        match self.proc.poll() {
            Some(status) => {
                if status.success() {
                    Ok(true)
                } else {
                    Err(string_error::new_err("task failed"))
                }
            },
            None => Ok(false),
        }
    }
}
