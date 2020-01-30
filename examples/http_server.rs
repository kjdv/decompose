#[macro_use]
extern crate rouille;
extern crate clap;

use std::fmt::Write;

fn main() {
    let args = clap::App::new("http_server")
        .author("Klaas de Vries")
        .about("simple http server, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("port")
                .help("port to run on")
                .short("p")
                .long("port")
                .default_value("8080")
        )
        .arg(
            clap::Arg::with_name("host")
                .help("host to bind on")
                .short("h")
                .long("host")
                .default_value("127.0.0.1")
        )
        .arg(
            clap::Arg::with_name("extra")
                .help("you can add extra argumens, they will be ignored")
                .multiple(true)
        )
        .get_matches();

    let host = args.value_of("host").unwrap();
    let port = args.value_of("port").unwrap();
    serve(format!("{}:{}", host, port).as_str());
}

fn serve(address: &str) {
    println!("listening at {}", address);

    rouille::start_server(address, move |request| {
        router!(request,
            (GET) (/hello) => {
                let hello = "hello!\n";

                print!("{}", hello);
                rouille::Response::text(hello)
            },
            (GET) (/args) => {
                let mut args = String::new();
                write!(&mut args, "args").unwrap();
                for arg in std::env::args().into_iter().skip(1) {
                    write!(&mut args, " {}", arg).unwrap();
                }
                write!(&mut args, "\n").unwrap();

                print!("{}", args);
                rouille::Response::text(args)
            },
            (GET) (/cwd) => {
                let cwd = std::env::current_dir().unwrap();
                let cwd = cwd.into_os_string();
                let cwd = cwd.into_string().unwrap();
                let cwd = format!("cwd {}\n", cwd);

                print!("{}", cwd);
                rouille::Response::text(cwd)
            },
            (GET) (/env) => {
                let mut env = String::new();
                write!(&mut env, "env\n").unwrap();
                for (key, value) in std::env::vars() {
                    write!(&mut env, "{}={}\n", key, value).unwrap();
                }

                print!("{}", env);
                rouille::Response::text(env)
            },
            _ => rouille::Response::empty_404()
        )
    })
}
