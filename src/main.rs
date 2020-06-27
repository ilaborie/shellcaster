use std::process;
use std::path::PathBuf;
use std::sync::mpsc;

mod main_controller;
mod config;
mod keymap;
mod db;
mod ui;
mod types;
mod feeds;
mod downloads;
mod play_file;

use crate::main_controller::{MainController, MainMessage};
use crate::types::*;
use crate::config::Config;
use crate::ui::UiMsg;
use crate::feeds::FeedMsg;
use crate::downloads::DownloadMsg;

// Specifies how long, in milliseconds, to display messages at the
// bottom of the screen in the UI.
const MESSAGE_TIME: u64 = 5000;

/// Main controller for shellcaster program.
/// 
/// Setup involves connecting to the sqlite database (creating it if 
/// necessary), then querying the list of podcasts and episodes. This
/// is then passed off to the UI, which instantiates the menus displaying
/// the podcast info.
/// 
/// After this, the program enters a loop that listens for user keyboard
/// input, and dispatches to the proper module as necessary. User input
/// to quit the program breaks the loop, tears down the UI, and ends the
/// program.
fn main() {
    // SETUP -----------------------------------------------------------

    // figure out where config file is located -- either specified from
    // command line args, or using default config location for OS
    let args: Vec<String> = std::env::args().collect();
    let config_path = get_config_path(&args)
        .unwrap_or_else(|| {
            println!("Could not identify your operating system's default directory to store configuration files. Please specify paths manually using config.toml and use `-c` or `--config` flag to specify where config.toml is located when launching the program.");
            process::exit(1);
        });
    let config = Config::new(&config_path);

    let mut main_ctrl = MainController::new(config);


    // MAIN LOOP --------------------------------------------------------

    // wait for messages from the UI and other threads, and then process
    let mut message_iter = main_ctrl.rx_to_main.iter();
    while let Some(message) = message_iter.next() {
        match message {
            Message::Ui(UiMsg::Quit) => break,

            Message::Ui(UiMsg::AddFeed(url)) => {
                let tx_feeds_to_main = mpsc::Sender::clone(&main_ctrl.tx_to_main);
                let _ = feeds::spawn_feed_checker(tx_feeds_to_main, url, None);
            },

            Message::Feed(FeedMsg::NewData(pod)) =>
                main_ctrl.add_or_sync_data(pod, false),

            Message::Feed(FeedMsg::Error) =>
                main_ctrl.msg_to_ui("Error retrieving RSS feed.".to_string()),

            Message::Ui(UiMsg::Sync(pod_index)) =>
                main_ctrl.sync(Some(pod_index)),

            Message::Feed(FeedMsg::SyncData(pod)) =>
                main_ctrl.add_or_sync_data(pod, true),

            Message::Ui(UiMsg::SyncAll) =>
                main_ctrl.sync(None),

            Message::Ui(UiMsg::Play(pod_index, ep_index)) =>
                main_ctrl.play_file(pod_index, ep_index),

            Message::Ui(UiMsg::MarkPlayed(pod_index, ep_index, played)) =>
                main_ctrl.mark_played(pod_index, ep_index, played),

            Message::Ui(UiMsg::MarkAllPlayed(pod_index, played)) =>
                main_ctrl.mark_all_played(pod_index, played),

            // TODO: Stuck with this here for now because
            // `main_ctrl.download_manager.download_list()` requires
            // mutable borrow
            Message::Ui(UiMsg::Download(pod_index, ep_index)) => {
                let pod_title;
                {
                    let borrowed_podcast_list = main_ctrl.podcasts.borrow();
                    pod_title = borrowed_podcast_list[pod_index as usize].title.clone();
                }
                let episode = main_ctrl.podcasts
                    .clone_episode(pod_index, ep_index).unwrap();

                // add directory for podcast, create if it does not exist
                match main_ctrl.create_podcast_dir(pod_title.clone()) {
                    Ok(path) => main_ctrl.download_manager.download_list(
                        &[&episode], &path),
                    Err(_) => main_ctrl.msg_to_ui(
                        format!("Could not create dir: {}", pod_title)),
                }
            },

            // downloading can produce any one of these responses
            Message::Dl(DownloadMsg::Complete(ep_data)) =>
                main_ctrl.download_complete(ep_data),
            Message::Dl(DownloadMsg::ResponseError(_)) =>
                main_ctrl.msg_to_ui("Error sending download request.".to_string()),
            Message::Dl(DownloadMsg::ResponseDataError(_)) =>
                main_ctrl.msg_to_ui("Error downloading episode.".to_string()),
            Message::Dl(DownloadMsg::FileCreateError(_)) =>
                main_ctrl.msg_to_ui("Error creating file.".to_string()),
            Message::Dl(DownloadMsg::FileWriteError(_)) =>
                main_ctrl.msg_to_ui("Error writing file to disk.".to_string()),

            // TODO: Stuck with this here for now because
            // `main_ctrl.download_manager.download_list()` requires
            // mutable borrow
            Message::Ui(UiMsg::DownloadAll(pod_index)) => {
                // TODO: Try to do this without cloning the podcast...
                let podcast = main_ctrl.podcasts
                    .clone_podcast(pod_index).unwrap();
                let pod_title = podcast.title.clone();
                let borrowed_ep_list = podcast
                    .episodes.borrow();

                let mut episodes = Vec::new();
                for e in borrowed_ep_list.iter() {
                    episodes.push(e);
                }

                // add directory for podcast, create if it does not exist
                match main_ctrl.create_podcast_dir(pod_title.clone()) {
                    Ok(path) => main_ctrl.download_manager.download_list(
                        &episodes, &path),
                    Err(_) => main_ctrl.msg_to_ui(
                        format!("Could not create dir: {}", pod_title)),
                }
            },

            Message::Ui(UiMsg::Noop) => (),
        }
    }

    // CLEANUP ----------------------------------------------------------
    main_ctrl.tx_to_ui.send(MainMessage::UiTearDown).unwrap();
    main_ctrl.ui_thread.join().unwrap();  // wait for UI thread to finish teardown
}


/// Gets the path to the config file if one is specified in the command-
/// line arguments, or else returns the default config path for the
/// user's operating system.
/// Returns None if default OS config directory cannot be determined.
/// 
/// Note: Right now we only have one possible command-line argument,
/// specifying a config path. If the command-line API is
/// extended in the future, this will have to be refactored.
fn get_config_path(args: &[String]) -> Option<PathBuf> {
    return match args.len() {
        3 => Some(PathBuf::from(&args[2])),
        _ => {
            let default_config = dirs::config_dir();
            match default_config {
                Some(mut path) => {
                    path.push("shellcaster");
                    path.push("config.toml");
                    Some(path)
                },
                None => None,
            } 
        },
    };
}