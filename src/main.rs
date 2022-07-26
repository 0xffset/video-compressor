use std::{
    collections::HashMap,
    fmt::Display,
    fs::{DirEntry, File},
    io::{BufRead, BufReader, Error, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use regex::Regex;
use serde::{Deserialize, Serialize};

macro_rules! filetype_check {
    ($path:ident, $($type:literal),*) => {
        ($($path.ends_with($type)) ||*) && !($($path.ends_with(&($type.to_string() + "_x265.mp4"))) ||*)
    };
}

enum SkipReason {
    Metadata(Error),
    ReadDir(Error),
    FileType(Error),
    Extension,
    Override(Error),
    NotAFile,
    OpeningCompressedFile(Error),
}

impl Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use SkipReason::*;
        match self {
            Metadata(e) => write!(f, "Failed to read metadata: {e}"),
            ReadDir(e) => write!(f, "Failed to read directory: {e}"),
            FileType(e) => write!(f, "Failed to determine file type: {e}"),
            Extension => write!(f, "Failed to determine file extension"),
            Override(e) => write!(f, "Failed to override file: {e}"),
            NotAFile => write!(f, "Not a file"),
            OpeningCompressedFile(e) => {
                write!(f, "Failed to open compressed file to read size: {e}")
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Log {
    shrunk_files: HashMap<String, (u64, u64)>,
    added_files: HashMap<String, (u64, u64)>,
    skipped_files: HashMap<String, String>,

    #[serde(skip)]
    save_file: String,
}

impl Log {
    pub fn new(path: String) -> Self {
        let path = path + "/compression_log.json";
        if let Ok(log_file) = File::open(path.clone()) {
            match serde_json::from_reader::<BufReader<File>, Log>(BufReader::new(log_file)) {
                Ok(mut cache) => {
                    cache.save_file = path;
                    return cache;
                }
                Err(_) => {}
            };
        };

        // if file doesn't exist or problems while opening just create a new log and ignore it
        Log {
            shrunk_files: HashMap::new(),
            added_files: HashMap::new(),
            skipped_files: HashMap::new(),
            save_file: path,
        }
    }

    pub fn is_already_processed(&self, path: &String) -> bool {
        self.shrunk_files.contains_key(path)
    }

    pub fn mark_processed(&mut self, path: String, prev: u64, post: u64) {
        self.shrunk_files.insert(path.clone(), (prev, post));
        self.added_files.insert(path, (prev, post));
    }

    pub fn mark_skipped(&mut self, path: String, reason: SkipReason) {
        self.skipped_files.insert(path, reason.to_string());
    }

    fn display_filesize(size: u64) -> String {
        let mut size = size as f64;
        let mut unit = "B";
        if size > 1024.0 {
            size /= 1024.0;
            unit = "KB";
        }
        if size > 1024.0 {
            size /= 1024.0;
            unit = "MB";
        }
        if size > 1024.0 {
            size /= 1024.0;
            unit = "GB";
        }

        format!("{size:.2}{unit}")
    }

    pub fn print_status(&mut self) {
        let mut total_prev = 0;
        let mut total_post = 0;
        if !self.added_files.is_empty() {
            println!(" ==== ==== ==== ");
            for (path, (prev, post)) in &self.added_files {
                total_prev += prev;
                total_post += post;
                println!(
                    "Compressed `{path}`: {} -> {}",
                    Log::display_filesize(*prev),
                    Log::display_filesize(*post),
                );
            }
            self.added_files.clear();
            println!(" ==== ==== ==== \n");
        }

        if !self.skipped_files.is_empty() {
            println!(" ==== ==== ==== ");
            for (path, reason) in &self.skipped_files {
                println!("Skipped `{path}`: {}", reason);
            }
            self.skipped_files.clear();
            println!(" ==== ==== ==== \n");
        }

        if total_prev != 0 {
            println!(
                "Total compression: {} -> {}",
                Log::display_filesize(total_prev),
                Log::display_filesize(total_post),
            );
        }
    }

    pub fn save(&self) {
        if let Ok(mut log_file) = File::create(self.save_file.clone()) {
            if let Err(e) = log_file.write(serde_json::to_string(self).unwrap().as_bytes()) {
                panic!("Failed to save cache to {}: {e}", self.save_file);
            }
        };
    }
}

fn iterate_dir(path: &PathBuf, log: &mut Log) {
    let read_dir = match std::fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(e) => {
            log.mark_skipped(path.to_string_lossy().to_string(), SkipReason::ReadDir(e));
            return;
        }
    };

    for dir in read_dir {
        if let Ok(dir) = dir {
            let path = dir.path().to_string_lossy().to_string();
            let metadata = match dir.metadata() {
                Ok(metadata) => metadata,
                Err(e) => {
                    log.mark_skipped(path, SkipReason::Metadata(e));
                    continue;
                }
            };

            if !metadata.is_dir() {
                if !log.is_already_processed(&path) {
                    let prev_size = metadata.len();
                    if let Ok(post_size) = process_entry(&dir, log) {
                        log.mark_processed(path, prev_size, post_size);
                        log.save();
                    }
                }
            } else {
                iterate_dir(&dir.path(), log);
            }
        }
    }
}

fn print_video_length(path_buf: PathBuf) {
    let stdout = match Command::new("ffprobe")
        .arg("-loglevel")
        .arg("fatal")
        .arg("-i")
        .arg(path_buf)
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("csv=p=0")
        .arg("-sexagesimal")
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => match child.stdout {
            Some(stdout) => stdout,
            None => return,
        },
        Err(_) => return,
    };

    let reader = BufReader::new(stdout);
    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| println!("Video length: {}", line));
}

fn compress(path_buf: PathBuf, dest_path_buf: PathBuf, log: &mut Log) {
    let stderr = match Command::new("ffmpeg")
        .arg("-loglevel")
        .arg("fatal")
        .arg("-stats")
        .arg("-i")
        .arg(path_buf)
        .arg("-c:v")
        .arg("libx265")
        .arg("-c:a")
        .arg("copy")
        .arg("-x265-params")
        .arg("crf=25")
        .arg("-x265-params")
        .arg("log-level=fatal")
        .arg(dest_path_buf)
        .arg("-y")
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => match child.stderr {
            Some(stderr) => stderr,
            None => {
                log.save();
                panic!("Failed to get ffmpeg stderr");
            }
        },
        Err(e) => {
            log.save();
            panic!("Failed to run ffmpeg: {e}");
        }
    };

    eprint!("Progress: 00:00:00");
    let time_regex = Regex::new(r"time=(\d+):(\d+):(\d+).(\d+)").unwrap();
    let mut second = 0;
    let mut minute = 0;
    let mut hour = 0;
    let mut buffer = String::new();
    for byte in stderr.bytes() {
        if let Ok(byte) = byte {
            buffer.push(byte as char);

            if time_regex.is_match(&buffer) {
                if let Some(captures) = time_regex.captures(&buffer) {
                    let new_second = captures[3].parse::<u64>().unwrap();
                    let new_minute = captures[2].parse::<u64>().unwrap();
                    let new_hour = captures[1].parse::<u64>().unwrap();
                    if new_hour > hour {
                        hour = new_hour;
                        minute = new_minute;
                        second = new_second;
                        eprint!("\rProgress: {hour:0>2}:{minute:0>2}:{second:0>2}");
                    } else if new_minute > minute {
                        minute = new_minute;
                        second = new_second;
                        eprint!("\rProgress: {hour:0>2}:{minute:0>2}:{second:0>2}");
                    } else if new_second > second {
                        second = new_second;
                        eprint!("\rProgress: {hour:0>2}:{minute:0>2}:{second:0>2}");
                    }

                    buffer.clear();
                }
            }
        }
    }
    eprintln!();
}

fn process_entry(entry: &DirEntry, log: &mut Log) -> Result<u64, ()> {
    let path = entry.path().to_string_lossy().to_string();
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(e) => {
            log.mark_skipped(path.clone(), SkipReason::FileType(e));
            return Err(());
        }
    };

    if file_type.is_file() {
        let path_buf = entry.path();

        if filetype_check!(path, ".mp4", ".mov") {
            let mut dest_path_buf = path_buf.clone();
            dest_path_buf.set_file_name(
                dest_path_buf
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
                    + "_x265.mp4",
            );

            println!("Compressing {}...", path_buf.to_string_lossy());
            print_video_length(path_buf.clone());
            compress(path_buf.clone(), dest_path_buf.clone(), log);

            let post_size = match File::open(dest_path_buf.clone()) {
                Ok(file) => match file.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(e) => {
                        log.mark_skipped(path, SkipReason::Metadata(e));
                        return Err(());
                    }
                },
                Err(e) => {
                    log.mark_skipped(path, SkipReason::OpeningCompressedFile(e));
                    return Err(());
                }
            };

            if let Err(e) = Command::new("mv").arg(dest_path_buf).arg(path_buf).spawn() {
                log.mark_skipped(path.clone(), SkipReason::Override(e));
                return Err(());
            }

            return Ok(post_size);
        }
    } else {
        log.mark_skipped(path.clone(), SkipReason::NotAFile);
    }

    Err(())
}

fn main() {
    let path: Vec<String> = std::env::args().collect();
    if path.len() != 2 {
        println!("Usage: {} <path>", path[0]);
        std::process::exit(1);
    }

    let path = path[1].clone();
    let mut log = Log::new(path.clone());
    iterate_dir(&PathBuf::from(path), &mut log);
    log.print_status();
    log.save();
}
