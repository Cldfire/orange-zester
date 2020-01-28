use structopt::StructOpt;
use rpassword::read_password_from_tty;
use orange_zest::Zester;
use std::path::PathBuf;
use std::fs::File;
use std::io::prelude::*;

#[derive(StructOpt, Debug)]
#[structopt(name = "zester", version = "0.1", author = "Cldfire")]
struct Opts {
    // TODO: if these are not provided prompt for them and hide input
    /// OAuth token
    #[structopt(long)]
    oauth_token: Option<String>,
    /// Client ID
    #[structopt(long)]
    client_id: Option<String>,
    /// Output file
    #[structopt(short, long, parse(from_os_str), required = true)]
    output: PathBuf,
}

fn main() {
    let opt = Opts::from_args();
    // TODO: get rid of unwraps
    let zester = Zester::new(opt.oauth_token.unwrap(), opt.client_id.unwrap()).unwrap();
    let likes = zester.likes().unwrap();
    let json = serde_json::to_string_pretty(&likes).unwrap();

    let mut file = File::create(&opt.output).unwrap();
    file.write_all(json.as_bytes()).unwrap();
}
