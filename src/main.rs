use structopt::StructOpt;
use structopt::clap::arg_enum;
use rpassword::read_password_from_tty;
use enum_iterator::IntoEnumIterator;
use indicatif::{ProgressBar, ProgressStyle};
use orange_zest::{write_json, Zester, PlaylistsZestingEvent, LikesZestingEvent, LikesAudioZestingEvent};
use dotenv::dotenv;
use std::thread;
use std::time::Duration;
use std::env;
use std::path::PathBuf;
use std::fs;
use std::fs::File;
use std::io;

#[derive(StructOpt, Debug)]
enum Opts {
    /// Obtain JSON archives of meaningful data
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
        #[structopt(short, long, parse(from_os_str), required = true, value_name = "path")]
        output_folder: PathBuf,
        /// Data kinds to get
        #[structopt(
            possible_values = &JsonType::variants(),
            case_insensitive = true,
            required_unless("all"),
            min_values = 1
        )]
        json_types: Vec<JsonType>
    },
    /// Obtain audio specified by pre-obtained JSON archives
    Audio {
        /// OAuth token
        #[structopt(long)]
        oauth_token: Option<String>,
        /// Client ID
        #[structopt(long)]
        client_id: Option<String>,
        /// Only get n most recent items
        #[structopt(short, long, value_name = "n")]
        recent: Option<u64>,
        /// Download all available audio (playlists, likes, etc.)
        #[structopt(short, long)]
        all: bool,
        /// Output folder
        #[structopt(short, long, parse(from_os_str), required = true, value_name = "path")]
        output_folder: PathBuf,
        /// Input folder from which to obtain JSON
        #[structopt(short, long, parse(from_os_str), required = true, value_name = "path")]
        input_folder: PathBuf,
        /// Audio kinds to get
        #[structopt(
            possible_values = &AudioType::variants(),
            case_insensitive = true,
            required_unless("all"),
            min_values = 1
        )]
        audio_types: Vec<AudioType>
    }
}

arg_enum! {
    #[derive(Debug, IntoEnumIterator)]
    enum JsonType {
        Likes,
        Me,
        Playlists,
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
    IoError(std::io::Error),
    /// No JSON file present at path
    JsonFileNotFound(String)
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

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(120);

    let tick_strings = &[
        "▹▹▹▹▹",
        "▸▹▹▹▹",
        "▹▸▹▹▹",
        "▹▹▸▹▹",
        "▹▹▹▸▹",
        "▹▹▹▹▸",
        "▪▪▪▪▪",
    ];
    let spinner_style = ProgressStyle::default_spinner()
        .tick_strings(tick_strings)
        .template("{spinner:.blue} {msg:.bold}");
    let bar_style = ProgressStyle::default_bar()
        .tick_strings(tick_strings)
        .progress_chars("#>-")
        .template("{spinner:.blue} {msg:<34!} [{bar:30.cyan/blue}] ({pos}/{len}) ({eta})");
    let bar_style_prefix = ProgressStyle::default_bar()
        .tick_strings(tick_strings)
        .progress_chars("#>-")
        .template("{spinner:.blue} {prefix:.bold}\n{msg:<40!} [{bar:30.cyan/blue}] ({pos}/{len}) ({eta})");

    pb.set_style(
        spinner_style.clone()
    );

    match opt {
        Opts::Json { mut oauth_token, mut client_id, all, pretty_print, output_folder, mut json_types } => {
            ensure_secrets_present(&mut oauth_token, &mut client_id)?;

            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                json_types = JsonType::into_enum_iter().collect();
            }

            pb.set_message("Creating zester");
            let zester = Zester::new(oauth_token.unwrap(), client_id.unwrap())?;
            pb.println("Zester created");

            // Grab all the data we were asked to
            for json_type in json_types {
                match json_type {
                    JsonType::Likes => {
                        use LikesZestingEvent::*;

                        pb.set_style(bar_style.clone());
                        pb.set_message("Zesting likes");
                        let total_likes_count = zester.me.as_ref().unwrap().likes_count.unwrap();
                        pb.set_length(total_likes_count as u64);

                        let mut path = output_folder.clone();
                        path.push("likes.json");
                        let likes = zester.likes(Some(|e| match e {
                            MoreLikesInfoDownloaded { count } => {
                                pb.inc(count as u64);
                            },

                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                                thread::sleep(Duration::from_secs(time_secs));
                                pb.set_message("Zesting likes");
                            }
                        }))?;
                        write_json(&likes, &path, pretty_print)?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested likes");
                    },
                    JsonType::Me => {
                        pb.set_message("Zesting profile information");

                        let mut path = output_folder.clone();
                        path.push("me.json");
                        let me = zester.me()?;
                        write_json(&me, &path, pretty_print)?;

                        pb.println("Zested profile information");
                    },
                    JsonType::Playlists => {
                        use orange_zest::PlaylistsZestingEvent::*;

                        pb.set_style(bar_style_prefix.clone());
                        pb.set_prefix("Zesting playlists");
                        pb.set_message("Getting list of playlists");
                        let total_playlist_count = zester.me.as_ref().unwrap().total_playlist_count();
                        pb.set_length(total_playlist_count as u64);

                        let mut path = output_folder.clone();
                        path.push("playlists.json");
                        let playlists = zester.playlists(Some(|e: PlaylistsZestingEvent<'_>| match e {
                            MorePlaylistMetaInfoDownloaded { count } => {
                                pb.inc(count as u64);
                            },
                            FinishPlaylistMetaInfoDownloading => {
                                pb.set_message("");
                                pb.reset();
                            },
                            StartPlaylistInfoDownload { playlist_meta } => {
                                pb.set_message(playlist_meta.title.as_ref().unwrap());
                            },
                            FinishPlaylistInfoDownload => {
                                pb.inc(1);
                            },
                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                            }
                        }))?;

                        write_json(&playlists, &path, pretty_print)?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested playlists");
                    }
                }
            }
        },

        Opts::Audio { mut oauth_token, mut client_id, recent, all, output_folder, input_folder, mut audio_types } => {
            ensure_secrets_present(&mut oauth_token, &mut client_id)?;

            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                audio_types = AudioType::into_enum_iter().collect();
            }

            // TODO: pull this block outside this match
            // will need a second match to handle tokens first
            pb.set_message("Creating zester");
            let zester = Zester::new(oauth_token.unwrap(), client_id.unwrap())?;
            pb.println("Zester created");
            pb.set_message("");

            // Grab all the data we were asked to
            for audio_type in audio_types {
                match audio_type {
                    AudioType::Likes => {
                        use LikesAudioZestingEvent::*;

                        let mut input_file = input_folder.clone();
                        input_file.push("likes.json");

                        let mut likes_folder = output_folder.clone();
                        likes_folder.push("likes/");
                        if !likes_folder.exists() {
                            fs::create_dir(&likes_folder)?;
                        }

                        pb.set_style(bar_style_prefix.clone());
                        pb.set_prefix("Zesting likes audio");

                        zester.likes_audio(input_file, recent.unwrap_or(std::u64::MAX), |e| match e {
                            NumTracksToDownload { num } => {
                                pb.set_length(num);
                            },

                            StartTrackDownload { track_info } => {
                                pb.set_message(track_info.title.as_ref().unwrap());
                            },

                            FinishTrackDownload { track_info, mut track_data } => {
                                let mut output_file = likes_folder.clone();
                                let title = track_info.title.as_ref().unwrap();
                                output_file.push(format!("{} (id={}).m4a", title, track_info.id.unwrap()));

                                // TODO: handle errors
                                let mut file = File::create(&output_file).unwrap();
                                io::copy(&mut track_data, &mut file).unwrap();
                                pb.inc(1);
                            },

                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                            }
                        })?;

                        // TODO: test json not being present

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested audio tracks from likes");
                    }
                }
            }
        }
    }

    pb.finish_with_message("Zesting complete");
    Ok(())
}
