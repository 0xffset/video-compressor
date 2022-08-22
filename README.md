## Video compressor

Small program that uses ffmpeg to compress videos (intended to compress lecture videos taking up all my systems space) using the x265 encoding.

It will recursively search through all subfolders for videos and compress them, replacing the original with the compressed version. While doing so, the program will produce a `compression_log.json` file that keeps track of the videos that were compressed or read one if it already exists in the base directory.
It is now possible to supply the path to a single video file and it will compress only that single file (it will still create the compression_log.json).

While the program is running it will show you the current video it is working on and the progress it has made. When the program has finished iterating over all items and sub directories it will print an overview of the compression and the compression rates, skipped files and any errors. The program can be interrupted and it will continue where it left off at the next start. (It only prints the overview if it is completely finished, interupting the program will not print the overview)

The program __will not__:
- compress videos that are already compressed
- search for a `compression_log.json` file in any parent or child folder

This program might not work on windows systems as i have not tested it on windows.

## Requirements
- Rust
- ffmpeg
- ffprobe (should be installed with ffmpeg)

### Usage
The program takes a `path` parameter that points to the folder containing the lecture videos. (I set it to my base uni folder so it can compress all the videos of different courses in the uni folder and I would advice to do the same)

```bash
```bash
$ cargo run --release <path>
```
