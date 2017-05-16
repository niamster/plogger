#[macro_use]
extern crate woodpecker;
use woodpecker as wp;
use wp::handlers::{rotating_file, stdout};

extern crate argparse;
use argparse::{ArgumentParser, StoreTrue, Store};

#[macro_use]
extern crate lazy_static;

extern crate libc;

extern crate mio;
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::unix::{EventedFd, UnixReady};

use std::ops::Deref;
use std::path::Path;
use std::io::{Write, Read};
use std::process;
use std::fs::File;
use std::os::unix::io::FromRawFd;

lazy_static! {
    static ref B_SUFFIX: Vec<&'static str> = vec!["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    static ref B_SUFFIX_LOWER: Vec<String> = B_SUFFIX.iter().map(|x| x.to_lowercase()).collect();
}

const BUF_SIZE: usize = 4096;
const NL: u8 = '\n' as u8;
const CR: u8 = '\r' as u8;

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

pub fn set_nonblock(fd: libc::c_int) -> Result<(), ()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        let res = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        if res == -1 {
            Err(())
        } else {
            Ok(())
        }
    }
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

    let mut stdin = unsafe { File::from_raw_fd(libc::STDIN_FILENO) };
    set_nonblock(libc::STDIN_FILENO).unwrap();

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1);
    let token = Token(0);

    poll.register(&EventedFd(&libc::STDIN_FILENO),
                  token,
                  Ready::readable() | UnixReady::hup(),
                  PollOpt::edge() | PollOpt::level()
    ).unwrap();

    let mut buffer = Vec::new();
    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            if token != event.token() {
                continue;
            }

            let readiness = event.readiness();

            if readiness.is_readable() {
                let mut chunk = [0; 1];
                match stdin.read_exact(&mut chunk) {
                    Ok(_) => {
                        let ch = chunk[0];
                        if ch == CR {
                            continue;
                        }
                        buffer.push(ch);
                        if ch == NL || buffer.len() == BUF_SIZE {
                            log!("{}", String::from_utf8_lossy(&buffer));
                            buffer.clear();
                        }
                    },
                    _ => {
                        break;
                    },
                }
                continue;
            }

            if UnixReady::from(readiness).is_hup()  {
                if buffer.len() > 0 {
                    log!("{}", String::from_utf8_lossy(&buffer));
                }
                return;
            }
        }
    }
}
