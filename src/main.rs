#[macro_use]
extern crate woodpecker;
use woodpecker as wp;
use wp::handlers::{rotating_file, stdout};

extern crate argparse;
use argparse::{ArgumentParser, StoreTrue, Store};

#[macro_use]
extern crate lazy_static;

use std::ops::Deref;
use std::path::Path;
use std::io::Write;
use std::process;

lazy_static! {
    static ref B_SUFFIX: Vec<&'static str> = vec!["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    static ref B_SUFFIX_LOWER: Vec<String> = B_SUFFIX.iter().map(|x| x.to_lowercase()).collect();
}

#[derive(Debug)]
enum ParseError {
    Invalid,
}

fn unbytify(value: &String) -> Result<u64, ParseError> {
    let value = value.to_lowercase();
    let value = value.trim();

    // w/o any suffix
    match value.parse() {
        Ok(x) => return Ok(x),
        _ => {},
    }

    for (idx, suffix) in B_SUFFIX_LOWER.iter().enumerate().rev() {
        let sfx = &suffix[..1];
        let mut res = value.split(sfx);

        let val: f64 = match res.next() {
            Some(x) => {
                match x.parse() {
                    Ok(x) => x,
                    _ => continue,
                }
            },
            _ => continue,
        };

        match res.next() {
            Some(rest) => {
                if rest.len() > 0 && rest != "b" && rest != "ib" {
                    return Err(ParseError::Invalid);
                }
            },
            _ => {},
        };

        let val = val * 1024u64.pow(idx as u32) as f64;
        return Ok(val.round() as u64);
    }

    Err(ParseError::Invalid)
}

fn main() {
    wp::init();

    let mut count = 10;
    let mut size = "1M".to_string();
    let mut stdout = false;
    let mut date = false;
    let mut file = String::new();

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Pipe logger");
        ap.refer(&mut stdout).add_option(&["--stdout"], StoreTrue, "Log to stdout");
        ap.refer(&mut date).add_option(&["-d", "--date"], StoreTrue, "Insert a timestamp before the log message");
        ap.refer(&mut count).add_option(&["-c", "--count"], Store, "Max number of log files");
        ap.refer(&mut size).add_option(&["-s", "--size"], Store, "Max size of a single log file");
        ap.refer(&mut file).add_argument("file", Store, "Log file name");
        ap.parse_args_or_exit();
    }

    let size: u64 = match unbytify(&size) {
        Ok(val) if val > 0 => val,
        _ => {
            writeln!(&mut std::io::stderr(), "Invalid size").unwrap();
            process::exit(2);
        }
    };

    if stdout {
        wp_register_handler!(stdout::handler());
    }

    if date {
        wp_set_formatter!(Box::new(|record| {
            format!("{} {}", record.ts_utc(), record.msg())
        }));
    } else {
        wp_set_formatter!(Box::new(|record| {
            record.msg().deref().clone()
        }));
    }

    wp_register_handler!(rotating_file::handler(Path::new(&file), count, size));

    let stdin = std::io::stdin();
    loop {
        let mut buffer = String::new();
        match stdin.read_line(&mut buffer) {
            Ok(count) if count > 0 => {
                log!("{}", buffer);
            },
            _ => {
                break;
            },
        }
    }
}
