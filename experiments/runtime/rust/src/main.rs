use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const GENERIC_ERROR: &str = "runtime experiment failed";

#[derive(Debug)]
struct Record {
    id: u64,
    account_id: u8,
    utc_seconds: i64,
    _unread: bool,
    subject: String,
    _sender: String,
    snippet: String,
}

fn parse_account(value: &str) -> Option<u8> {
    match value {
        "1" => Some(1),
        "2" => Some(2),
        "3" => Some(3),
        "4" => Some(4),
        "5" => Some(5),
        _ => None,
    }
}

fn parse_cache(path: &Path) -> Result<Vec<Record>, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(());
    }
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    let mut records = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 7 || fields.iter().any(|field| field.contains('\r')) {
            return Err(());
        }
        let id = fields[0].parse::<u64>().map_err(|_| ())?;
        if id == 0 {
            return Err(());
        }
        let account_id = parse_account(fields[1]).ok_or(())?;
        let utc_seconds = fields[2].parse::<i64>().map_err(|_| ())?;
        let unread = match fields[3] {
            "0" => false,
            "1" => true,
            _ => return Err(()),
        };
        if fields[4].contains('\n') || fields[5].contains('\n') || fields[6].contains('\n') {
            return Err(());
        }
        records.push(Record {
            id,
            account_id,
            utc_seconds,
            _unread: unread,
            subject: fields[4].to_owned(),
            _sender: fields[5].to_owned(),
            snippet: fields[6].to_owned(),
        });
    }
    records.sort_by(|left, right| {
        right
            .utc_seconds
            .cmp(&left.utc_seconds)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(records)
}

fn write_line(output: &mut impl Write, line: &str) -> io::Result<()> {
    output.write_all(line.as_bytes())?;
    output.write_all(b"\n")?;
    output.flush()
}

fn list_response(records: &[Record], account: u8) -> String {
    let ids = records
        .iter()
        .filter(|record| record.account_id == account)
        .take(100)
        .map(|record| record.id.to_string())
        .collect::<Vec<_>>();
    format!("LIST\t{}\t{}", ids.len(), ids.join(","))
}

fn search_response(records: &[Record], account: u8, query: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    let mut count = 0_u64;
    let mut sum = 0_u64;
    for record in records.iter().filter(|record| record.account_id == account) {
        if record.subject.contains(query) || record.snippet.contains(query) {
            count = count.checked_add(1)?;
            sum = sum.checked_add(record.id)?;
        }
    }
    Some(format!("SEARCH\t{count}\t{sum}"))
}

fn body_response(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let sum = bytes
        .iter()
        .try_fold(0_u64, |total, byte| total.checked_add(u64::from(*byte)))?;
    Some(format!("BODY\t{}\t{sum}", bytes.len()))
}

fn serve(cache_path: &Path, body_path: &Path) -> Result<(), ()> {
    let records = parse_cache(cache_path)?;
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    write_line(&mut output, &format!("READY\t{}", records.len())).map_err(|_| ())?;

    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).map_err(|_| ())? == 0 {
            return Ok(());
        }
        while line.ends_with(['\n', '\r']) {
            line.pop();
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let response = match fields.as_slice() {
            ["LIST", account] => parse_account(account).map(|id| list_response(&records, id)),
            ["SEARCH", account, query] => {
                parse_account(account).and_then(|id| search_response(&records, id, query))
            }
            ["BODY"] => body_response(body_path),
            ["PING"] => Some("PONG".to_owned()),
            ["QUIT"] => {
                write_line(&mut output, "BYE").map_err(|_| ())?;
                return Ok(());
            }
            _ => None,
        };
        write_line(&mut output, response.as_deref().unwrap_or("ERR")).map_err(|_| ())?;
    }
}

fn run() -> Result<(), ()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    match (args.next(), args.next(), args.next(), args.next()) {
        (Some(command), None, None, None) if command == "--ping" => {
            let mut stdout = io::BufWriter::new(io::stdout().lock());
            write_line(&mut stdout, "PONG").map_err(|_| ())
        }
        (Some(command), Some(cache), Some(body), None) if command == "--serve" => {
            serve(&PathBuf::from(cache), &PathBuf::from(body))
        }
        _ => Err(()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            let _ = writeln!(io::stderr().lock(), "{GENERIC_ERROR}");
            ExitCode::from(1)
        }
    }
}
