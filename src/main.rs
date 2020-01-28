use structopt::StructOpt;
use structopt::clap::arg_enum;
use rpassword::read_password_from_tty;
use enum_iterator::IntoEnumIterator;
use orange_zest::Zester;
use std::path::PathBuf;
use std::fs::File;
use std::io::prelude::*;

#[derive(StructOpt, Debug)]
enum Opts {
    Json {
        /// OAuth token
        #[structopt(long)]
        oauth_token: Option<String>,
        /// Client ID
        #[structopt(long)]
        client_id: Option<String>,
        /// Download all available data (archive everything)
        #[structopt(short, long)]
        all: bool,
        /// Pretty print the JSON output
        #[structopt(short, long)]
        pretty_print: bool,
        /// Output folder
        #[structopt(short, long, parse(from_os_str), required = true)]
        output: PathBuf,
        /// The kind(s) of data to get
        #[structopt(
            possible_values = &JsonType::variants(),
            case_insensitive = true,
            required_unless("all"),
            min_values = 1)
        ]
        json_types: Vec<JsonType>
    }
}

arg_enum! {
    #[derive(Debug, IntoEnumIterator)]
    enum JsonType {
        Likes,
        Me,
    }
}

// TODO: get rid of unwraps
fn main() {
    let opt = Opts::from_args();

    match opt {
        Opts::Json { mut oauth_token, mut client_id, all, pretty_print, output, mut json_types } => {
            if oauth_token.is_none() {
                oauth_token = Some(read_password_from_tty(Some("OAuth token: ")).unwrap());
            }
            if client_id.is_none() {
                client_id = Some(read_password_from_tty(Some("Client ID: ")).unwrap());
            }

            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                json_types = JsonType::into_enum_iter().collect();
            }

            let zester = Zester::new(oauth_token.unwrap(), client_id.unwrap()).unwrap();

            // Grab all the data we were asked to
            for json_type in json_types {
                let json;
                let file_name;

                match json_type {
                    JsonType::Likes => {
                        json = if pretty_print {
                            serde_json::to_string_pretty(&zester.likes().unwrap()).unwrap()
                        } else {
                            serde_json::to_string(&zester.likes().unwrap()).unwrap()
                        };

                        file_name = "likes";
                    },
                    JsonType::Me => {
                        json = if pretty_print {
                            serde_json::to_string_pretty(&zester.me().unwrap()).unwrap()
                        } else {
                            serde_json::to_string(&zester.me().unwrap()).unwrap()
                        };

                        file_name = "me";
                    }
                }

                // Write the json to disk
                let mut path = output.clone();
                path.push(file_name);
                path.set_extension("json");

                let mut file = File::create(path).unwrap();
                file.write_all(json.as_bytes()).unwrap();
            }
        }
    }


}
