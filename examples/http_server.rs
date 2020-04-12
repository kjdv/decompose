#[macro_use]
extern crate rouille;
extern crate clap;

fn main() {
    let args = clap::App::new("http_server")
        .author("Klaas de Vries")
        .about("simple http server, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("address")
                .help("address to bind to")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:8080"),
        )
        .arg(
            clap::Arg::with_name("extra")
                .help("you can add extra argumens, they will be ignored")
                .multiple(true),
        )
        .get_matches();

    let address = args.value_of("address").unwrap();
    serve(address);
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
                request.get_param("idx")
                    .ok_or("no idx param provided")
                    .and_then(|idx| {
                        idx.parse::<usize>()
                            .map_err(|_| "invalid index")
                    })
                    .and_then(|idx| {
                        std::env::args().nth(idx)
                            .ok_or("out of range")
                    })
                    .map_or_else(|err| {
                        println!("{}", err);
                        rouille::Response::text(err).with_status_code(400)
                    }, |arg| {
                        println!("{}", arg);
                        rouille::Response::text(arg)
                    })
            },
            (GET) (/cwd) => {
                let cwd = std::env::current_dir().unwrap();
                let cwd = cwd.into_os_string();
                let cwd = cwd.into_string().unwrap();

                println!("{}", cwd);
                rouille::Response::text(cwd)
            },
            (GET) (/env) => {
                request.get_param("key")
                    .ok_or("no key provided")
                    .and_then(|key| {
                        std::env::var(key)
                            .map_err(|_| "no such variable")
                    })
                    .map_or_else(|err| {
                        println!("{}", err);
                        rouille::Response::text(err).with_status_code(400)
                    }, |val| {
                        println!("{}", val);
                        rouille::Response::text(val)
                    })
            },
            _ => rouille::Response::empty_404()
        )
    })
}
