//! png_to_svg - vectorize PNG files into SVG with clean edges.
//!
//! Usage: png_to_svg [--no-smooth] image.png [more.png ...]
//!
//! Each SVG is written next to its PNG with the same name
//! (image.png -> image.svg). An existing SVG with that name is overwritten.

mod smooth;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;

const USAGE: &str = "\
Usage: png_to_svg [--no-smooth] image.png [more.png ...]

Vectorizes each PNG and saves the SVG next to it with the same name.
  --no-smooth   keep vtracer's raw output (skip the edge cleanup)
  -h, --help    show this help";

/// Desktop notification, used when launched from a file manager (no terminal).
fn notify(title: &str, message: &str) {
    if std::io::stdout().is_terminal() {
        return;
    }
    // Ignore errors: notify-send may simply not be installed.
    let _ = Command::new("notify-send").arg(title).arg(message).status();
}

/// Run vtracer. Done on a thread with a large stack (deep recursion on big
/// images) and with panics caught so one bad file can't stop a batch.
fn trace(src: PathBuf, dst: PathBuf) -> Result<(), String> {
    let handle = thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || {
            let mut config = vtracer::Config::default();
            config.path_precision = Some(3);
            vtracer::convert_image_to_svg(&src, &dst, config)
        })
        .map_err(|e| format!("could not start worker thread: {e}"))?;

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("vtracer crashed while tracing this image".to_string()),
    }
}

fn convert(src: &Path, smooth: bool) -> Result<PathBuf, String> {
    let dst = src.with_extension("svg");
    let mut part_name: OsString = dst
        .file_name()
        .ok_or_else(|| "bad file name".to_string())?
        .to_os_string();
    part_name.push(".part");
    let part = dst.with_file_name(part_name);

    let _ = fs::remove_file(&part);
    if let Err(e) = trace(src.to_path_buf(), part.clone()) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }

    if smooth {
        match fs::read_to_string(&part) {
            Ok(raw) => {
                let cleaned = smooth::smooth_svg(&raw, smooth::DEFAULT_TOLERANCE);
                if let Err(e) = fs::write(&part, cleaned) {
                    eprintln!("  note: edge smoothing skipped ({e})");
                }
            }
            Err(e) => eprintln!("  note: edge smoothing skipped ({e})"),
        }
    }

    // Replace the final file only once the new one is complete.
    fs::rename(&part, &dst).map_err(|e| format!("could not save {}: {e}", dst.display()))?;
    Ok(dst)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let smooth = !args.iter().any(|a| a == "--no-smooth");
    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| a.as_str() != "--no-smooth")
        .map(PathBuf::from)
        .collect();

    if files.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let (mut ok, mut failed) = (0usize, 0usize);
    for src in &files {
        let is_png = src
            .extension()
            .map(|e| e.eq_ignore_ascii_case("png"))
            .unwrap_or(false);
        let result = if !src.is_file() {
            Err(format!("not found: {}", src.display()))
        } else if !is_png {
            Err("not a PNG".to_string())
        } else {
            convert(src, smooth)
        };

        match result {
            Ok(dst) => {
                println!("{} -> {}", src.display(), dst.display());
                ok += 1;
            }
            Err(e) => {
                eprintln!("{}: FAILED: {e}", src.display());
                let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                notify("PNG to SVG failed", &format!("{name}: {e}"));
                failed += 1;
            }
        }
    }

    if ok > 0 {
        notify(
            "PNG to SVG",
            &format!("Converted {ok} file{}", if ok == 1 { "" } else { "s" }),
        );
    }
    if failed > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
