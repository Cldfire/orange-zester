use structopt::StructOpt;
use structopt::clap::arg_enum;
use rpassword::read_password_from_tty;
use enum_iterator::IntoEnumIterator;
use indicatif::{ProgressBar, ProgressStyle};
use orange_zest::{write_json, Zester};
use orange_zest::api::{Likes, Playlists};
use orange_zest::events::*;
use dotenv::dotenv;
use std::thread;
use std::cell::RefCell;
use std::time::Duration;
use std::env;
use std::path::{Path, PathBuf};
use std::fs;
use std::fs::File;
use std::io;
use std::io::Read;

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

impl Opts {
    /// Takes the tokens out of this `Opts` instance and hands them to you.
    fn tokens(&mut self) -> (Option<String>, Option<String>) {
        match self {
            Opts::Json { oauth_token, client_id, .. } => 
                (oauth_token.take(), client_id.take()),
            Opts::Audio { oauth_token, client_id, .. } => 
                (oauth_token.take(), client_id.take())
        }
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
        Playlists
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

// Sanitize the given filename for storage across different OS's
fn sanitize<S: AsRef<str>>(name: S) -> String {
    sanitize_filename::sanitize_with_options(
        name,
        sanitize_filename::Options {
            windows: true,
            .. Default::default()
        }
    )
}

// If the given generic error is an `io::ErrorKind::NotFound`, turn it into a
// `JsonFileNotFound`.
fn specific_json_err(generic_err: orange_zest::Error, filepath: String) -> Error {
    match generic_err {
        orange_zest::Error::IoError(e) => match e.kind() {
            io::ErrorKind::NotFound => Error::JsonFileNotFound(filepath),
            _ => e.into()
        },
        _ => generic_err.into()
    }
}

// Streams the given `Read` instance to the given file path.
//
// Handles pretty-printing relevant errors.
fn stream_track_to_file<P: AsRef<Path>>(path: P, track_title: &str, pb: &ProgressBar, mut data: impl Read) {
    match File::create(path.as_ref()) {
        Ok(mut f) => match io::copy(&mut data, &mut f) {
            Ok(_) => {},
            Err(e) => {
                pb.println(&format!("  [warning] Failed to write \"{}\" to file: {}", track_title, e));
            }
        },
        Err(e) => {
            pb.println(&format!("  [warning] Failed to create {}: {}", path.as_ref().display(), e));
        }
    };
}

fn main() -> Result<(), Error> {
    let mut opt = Opts::from_args();
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

    let zester;
    {
        let (mut oauth_token, mut client_id) = opt.tokens();
        ensure_secrets_present(&mut oauth_token, &mut client_id)?;

        pb.set_message("Creating zester");
        zester = Zester::new(oauth_token.unwrap(), client_id.unwrap())?;
        pb.println("Zester created");
    }

    match opt {
        Opts::Json { all, pretty_print, output_folder, mut json_types, .. } => {
            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                json_types = JsonType::into_enum_iter().collect();
            }

            // Grab all the data we were asked to
            for json_type in json_types {
                match json_type {
                    JsonType::Likes => {
                        use LikesZestingEvent::*;

                        pb.set_style(bar_style.clone());
                        pb.set_message("Zesting likes");
                        let total_likes_count = zester.me.as_ref().unwrap().likes_count.unwrap();
                        pb.set_length(total_likes_count as u64);

                        let path = output_folder.join("likes.json");
                        let likes = zester.likes(|e| match e {
                            MoreLikesInfoDownloaded { count } => {
                                pb.inc(count as u64);
                            },

                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                                thread::sleep(Duration::from_secs(time_secs));
                                pb.set_message("Zesting likes");
                            }
                        })?;
                        write_json(&likes, &path, pretty_print)?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested likes");
                    },
                    JsonType::Me => {
                        pb.set_message("Zesting profile information");

                        let path = output_folder.join("me.json");
                        let me = zester.me()?;
                        write_json(&me, &path, pretty_print)?;

                        pb.println("Zested profile information");
                    },
                    JsonType::Playlists => {
                        use PlaylistsZestingEvent::*;

                        pb.set_style(bar_style_prefix.clone());
                        pb.set_prefix("Zesting playlists");
                        pb.set_message("Getting list of playlists");
                        let total_playlist_count = zester.me.as_ref().unwrap().total_playlist_count();
                        pb.set_length(total_playlist_count as u64);

                        let path = output_folder.join("playlists.json");
                        let playlists = zester.playlists(|e: PlaylistsZestingEvent<'_>| match e {
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
                            FinishPlaylistInfoDownload { .. } => {
                                pb.inc(1);
                            },
                            PlaylistInfoDownloadError { playlist_meta, err } => {
                                pb.println(format!(
                                    "  [warning] failed to get info for {}: {:?}",
                                    playlist_meta.title.as_ref().unwrap(),
                                    err
                                ));
                                pb.inc(1);
                            },
                            PlaylistInfoCompletionError { playlist_meta, err } => {
                                pb.println(format!(
                                    "  [warning] failed to complete info for {}: {:?}",
                                    playlist_meta.title.as_ref().unwrap(),
                                    err
                                ));
                                pb.inc(1);
                            }
                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                            }
                        })?;

                        write_json(&playlists, &path, pretty_print)?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested playlists");
                    }
                }
            }
        },

        Opts::Audio { recent, all, output_folder, input_folder, mut audio_types, .. } => {
            // Manually stick all the possible types in the vector if the all flag
            // was set
            if all {
                audio_types = AudioType::into_enum_iter().collect();
            }
            pb.set_message("");
            pb.set_style(bar_style_prefix.clone());

            let recent = recent.unwrap_or(std::u64::MAX);

            // Grab all the data we were asked to
            for audio_type in audio_types {
                match audio_type {
                    AudioType::Likes => {
                        use TracksAudioZestingEvent::*;
                        
                        let input_file = input_folder.join("likes.json");
                        let likes: Likes = orange_zest::load_json(&input_file)
                            .map_err(|e| specific_json_err(e, input_file.to_str().unwrap().into()))?;

                        let likes_folder = output_folder.join("likes/");
                        if !likes_folder.exists() {
                            fs::create_dir(&likes_folder)?;
                        }
                        pb.set_prefix("Zesting likes audio");

                        zester.likes_audio(&likes, recent, |e| match e {
                            NumTracksToDownload { num } => {
                                pb.set_length(num);
                            },

                            StartTrackDownload { track_info } => {
                                pb.set_message(track_info.title.as_ref().unwrap());
                            },

                            FinishTrackDownload { track_info, mut track_data } => {
                                let title = track_info.title.as_ref().unwrap();
                                let output_file = likes_folder.join(sanitize(format!(
                                    "{} (id={}).m4a",
                                    title,
                                    track_info.id.unwrap()
                                )));

                                stream_track_to_file(&output_file, &title, &pb, &mut track_data);
                                pb.inc(1);
                            },

                            TrackDownloadError { track_info, err } => {
                                pb.println(format!(
                                    "  [warning] failed to download {} {:?}",
                                    track_info.title.as_ref().unwrap(),
                                    err
                                ));
                                pb.inc(1);
                            },

                            PausedAfterServerError { time_secs } => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                            }
                        })?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested audio tracks from likes");
                    },

                    AudioType::Playlists => {
                        use PlaylistsAudioZestingEvent::*;
                        use TracksAudioZestingEvent::*;
                        
                        let input_file = input_folder.join("playlists.json");
                        let playlists: Playlists = orange_zest::load_json(&input_file)
                            .map_err(|e| specific_json_err(e, input_file.to_str().unwrap().into()))?;
                        // We need these refcells to track additional state for the progressbar
                        // that we can mutate from inside the Fn below
                        let playlist_curr = RefCell::new(1);
                        let playlist_total = RefCell::new(!0);

                        let playlists_folder = output_folder.join("playlists/");
                        if !playlists_folder.exists() {
                            fs::create_dir(&playlists_folder)?;
                        }
                        pb.set_prefix("Zesting playlists audio");

                        zester.playlists_audio(playlists.playlists.iter().take(recent as usize), |e| match e {
                            NumItemsToDownload { playlists_num, tracks_num } => {
                                *playlist_total.borrow_mut() = playlists_num;
                                pb.set_length(tracks_num);
                            },

                            StartPlaylistDownload { playlist_info } => {
                                pb.set_prefix(&format!(
                                    "Zesting playlists audio ({}/{}) - {}",
                                    playlist_curr.borrow(),
                                    playlist_total.borrow(),
                                    playlist_info.title.as_ref().unwrap()
                                ));
                            }

                            TrackEvent(NumTracksToDownload { .. }, _) => {},

                            TrackEvent(StartTrackDownload { track_info }, _) => {
                                pb.set_message(track_info.title.as_ref().unwrap());
                            },

                            TrackEvent(FinishTrackDownload { track_info, mut track_data }, playlist_info) => {
                                let track_title = track_info.title.as_ref().unwrap();
                                let playlist_title = playlist_info.title.as_ref().unwrap();

                                let playlist_folder = playlists_folder.join(sanitize(format!(
                                    "{} (id={})",
                                    playlist_title,
                                    playlist_info.id.unwrap(),
                                )));
                                if !playlist_folder.exists() {
                                    // TODO: don't unwrap
                                    fs::create_dir(&playlist_folder).unwrap();
                                }

                                let output_file = playlist_folder.join(sanitize(format!(
                                    "{} (id={}).m4a",
                                    track_title,
                                    track_info.id.unwrap()
                                )));

                                stream_track_to_file(&output_file, &track_title, &pb, &mut track_data);
                                pb.inc(1);
                            },

                            TrackEvent(TrackDownloadError { track_info, err }, playlist_info) => {
                                pb.println(format!(
                                    "  [warning] failed to download {} (in {}): {:?}",
                                    track_info.title.as_ref().unwrap(),
                                    playlist_info.title.as_ref().unwrap(),
                                    err
                                ));
                                pb.inc(1);
                            },

                            TrackEvent(PausedAfterServerError { time_secs }, _) => {
                                pb.set_message(&format!("Server error, retrying after {}s", time_secs));
                            },

                            FinishPlaylistDownload { playlist_info } => {
                                *playlist_curr.borrow_mut() += 1;
                                pb.set_prefix(&format!(
                                    "Zesting playlists audio ({}/{}) - {}",
                                    playlist_curr.borrow(),
                                    playlist_total.borrow(),
                                    playlist_info.title.as_ref().unwrap()
                                ));
                            }
                        })?;

                        pb.reset();
                        pb.set_style(spinner_style.clone());
                        pb.set_length(!0);
                        pb.println("Zested audio tracks from playlists");
                    }
                }
            }
        }
    }

    pb.finish_with_message("Zesting complete");
    Ok(())
}
