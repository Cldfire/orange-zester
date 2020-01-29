use structopt::StructOpt;
use structopt::clap::arg_enum;
use rpassword::read_password_from_tty;
use enum_iterator::IntoEnumIterator;
use indicatif::{ProgressBar, ProgressStyle};
use orange_zest::{load_json, write_json, Zester};
use orange_zest::api::Likes;
use dotenv::dotenv;
use std::env;
use std::path::PathBuf;
use std::fs::File;
use std::io;

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
        output_folder: PathBuf,
        /// The kind(s) of data to get
        #[structopt(
            possible_values = &JsonType::variants(),
            case_insensitive = true,
            required_unless("all"),
            min_values = 1)
        ]
        json_types: Vec<JsonType>
    },
    Audio {
        /// OAuth token
        #[structopt(long)]
        oauth_token: Option<String>,
        /// Client ID
        #[structopt(long)]
        client_id: Option<String>,
        /// Download all available audio (playlists, likes, etc.)
        #[structopt(short, long)]
        all: bool,
        /// Output folder
        #[structopt(short, long, parse(from_os_str), required = true)]
        output_folder: PathBuf,
        /// Input folder from which to obtain JSON
        #[structopt(short, long, parse(from_os_str), required = true)]
        input_folder: PathBuf,
        /// The kind(s) of audio to get
        #[structopt(
            possible_values = &AudioType::variants(),
            case_insensitive = true,
            required_unless("all"),
            min_values = 1)
        ]
        audio_types: Vec<AudioType>
    }
}

arg_enum! {
    #[derive(Debug, IntoEnumIterator)]
    enum JsonType {
        Likes,
        Me,
    }
}

arg_enum! {
    #[derive(Debug, IntoEnumIterator)]
    enum AudioType {
        Likes,
    }
}

#[derive(Debug)]
enum Error {
    OrangeZestError(orange_zest::Error),
    VarError(std::env::VarError),
    IoError(std::io::Error)
}

impl From<orange_zest::Error> for Error {
    fn from(err: orange_zest::Error) -> Self {
        Error::OrangeZestError(err)
    }
}

impl From<std::env::VarError> for Error {
    fn from(err: std::env::VarError) -> Self {
        Error::VarError(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err)
    }
}

// Attempt to fill the given secrets from the terminal or the environment if they
// are not already present
fn ensure_secrets_present(oauth_token: &mut Option<String>, client_id: &mut Option<String>) -> Result<(), Error> {
    if oauth_token.is_none() {
        if let Ok(token) = env::var("OAUTH_TOKEN") {
            *oauth_token = Some(token);
        } else {
            *oauth_token = Some(read_password_from_tty(Some("OAuth token: "))?);
        }
    }

    if client_id.is_none() {
        if let Ok(id) = env::var("CLIENT_ID") {
            *client_id = Some(id);
        } else {
            *client_id = Some(read_password_from_tty(Some("Client ID: "))?);
        }
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    let opt = Opts::from_args();
    dotenv().ok();

    match opt {
        Opts::Json { mut oauth_token, mut client_id, all, pretty_print, output_folder, mut json_types } => {
            ensure_secrets_present(&mut oauth_token, &mut client_id)?;

            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                json_types = JsonType::into_enum_iter().collect();
            }

            let pb = ProgressBar::new_spinner();
            pb.enable_steady_tick(120);
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_strings(&[
                        "▹▹▹▹▹",
                        "▸▹▹▹▹",
                        "▹▸▹▹▹",
                        "▹▹▸▹▹",
                        "▹▹▹▸▹",
                        "▹▹▹▹▸",
                        "▪▪▪▪▪",
                    ])
                    .template("{spinner:.blue} {msg}"),
            );

            pb.set_message("Creating zester");
            let zester = Zester::new(oauth_token.unwrap(), client_id.unwrap())?;
            pb.println("Zester created");

            // Grab all the data we were asked to
            for json_type in json_types {
                match json_type {
                    JsonType::Likes => {
                        pb.set_message("Zesting likes");

                        let mut path = output_folder.clone();
                        path.push("likes.json");
                        let likes = zester.likes()?;
                        write_json(&likes, &path, pretty_print)?;

                        pb.println("Zested likes");
                    },
                    JsonType::Me => {
                        pb.set_message("Zesting profile information");

                        let mut path = output_folder.clone();
                        path.push("me.json");
                        let me = zester.me()?;
                        write_json(&me, &path, pretty_print)?;

                        pb.println("Zested profile information");
                    }
                }
            }

            pb.finish_with_message("Zesting complete");
        },

        Opts::Audio { mut oauth_token, mut client_id, all, output_folder, input_folder, mut audio_types } => {
            ensure_secrets_present(&mut oauth_token, &mut client_id)?;

            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                audio_types = AudioType::into_enum_iter().collect();
            }

            let zester = Zester::new(oauth_token.unwrap(), client_id.unwrap())?;

            // Grab all the data we were asked to
            for audio_type in audio_types {
                match audio_type {
                    AudioType::Likes => {
                        let mut input_file = input_folder.clone();
                        input_file.push("likes.json");
                        let likes: Likes = load_json(&input_file)?;

                        // TODO: take(5) is for testing
                        for track in likes.collections.iter().map(|c| &c.track).take(5) {
                            let mut output_file = output_folder.clone();
                            // TODO: this could cause conflicts
                            output_file.push(track.title.as_ref().unwrap().clone() + ".m4a");

                            let mut file = File::create(&output_file)?;
                            io::copy(&mut track.download(&zester)?, &mut file)?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
