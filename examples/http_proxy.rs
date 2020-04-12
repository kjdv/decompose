extern crate clap;
extern crate reqwest;
extern crate rouille;

fn main() {
    let args = clap::App::new("http_server")
        .author("Klaas de Vries")
        .about("simple http forwding, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("address")
                .help("address to bind to")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:8081"),
        )
        .arg(
            clap::Arg::with_name("forward")
                .help("address to forwad to")
                .short("f")
                .long("forward")
                .default_value("127.0.0.1:8080"),
        )
        .arg(
            clap::Arg::with_name("extra")
                .help("you can add extra argumens, they will be ignored")
                .multiple(true),
        )
        .get_matches();

    let address = args.value_of("address").unwrap();
    let forward = args.value_of("forward").unwrap();
    serve(address, forward.to_string());
}

fn serve(address: &str, forward: String) {
    println!("listening at {}, forwarding to {}", address, forward);

    rouille::start_server(address, move |request| {
        let url = request.url();
        let to = format!("http://{}{}", forward, url);

        println!("forwarding {}", to);

        let res = reqwest::blocking::get(to.as_str()).unwrap();
        let status = res.status().as_u16();
        let body = res.text().unwrap();

        rouille::Response::text(body).with_status_code(status)
    })
}
