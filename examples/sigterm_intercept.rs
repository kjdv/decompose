extern crate clap;
extern crate tokio;

use std::error::Error;
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = clap::App::new("signal interceptor")
        .author("Klaas de Vries")
        .about("intercepts SIGINT and SIGTERM, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("diehard")
                .help("die hard, keep running on SIGINT or SIGTERM")
                .short("d")
                .long("diehard"),
        )
        .arg(
            clap::Arg::with_name("err")
                .help("print to stderr instead of stdout")
                .short("e")
                .long("err"),
        )
        .get_matches();

    let to_stderr = args.is_present("err");
    let printer = Printer { to_stderr };

    let ret = !args.is_present("diehard");

    let a = print_signal(SignalKind::terminate(), ret, "SIGTERM", to_stderr);
    let b = print_signal(SignalKind::interrupt(), ret, "SIGINT", to_stderr);

    tokio::select! {
        _ = a => (),
        _ = b => (),
    }

    printer.print("doing a clean exit".to_string());

    Ok(())
}

async fn print_signal(kind: SignalKind, ret: bool, name: &str, to_stderr: bool) {
    let printer = Printer { to_stderr };

    let mut stream = signal(kind).expect("signal");
    printer.print(format!("listening for {}", name));

    loop {
        stream.recv().await;
        printer.print(format!("Received signal {}", name));

        if ret {
            return;
        }
    }
}

struct Printer {
    to_stderr: bool,
}

impl Printer {
    fn print(&self, line: String) {
        if self.to_stderr {
            eprintln!("{}", line);
        } else {
            println!("{}", line)
        }
    }
}
