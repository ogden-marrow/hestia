//! One secret scanner, used everywhere something could leave the machine: capture, push, publish.
//! It reports which rule matched and where, never the secret itself. It reads bytes, so binary files
//! (token databases) are scanned too. Case does not matter.

use regex::bytes::Regex;
use std::sync::OnceLock;

pub struct Finding {
    pub rule: &'static str,
    pub line: usize,
}

const RULES: &[(&str, &str)] = &[
    ("private key", r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY"),
    ("github token", r"\bgh[pousr]_[A-Za-z0-9]{30,}"),
    ("github fine-grained token", r"\bgithub_pat_[A-Za-z0-9_]{30,}"),
    ("anthropic key", r"\bsk-ant-[A-Za-z0-9_\-]{20,}"),
    ("openai-style key", r"\bsk-(?:proj-)?[A-Za-z0-9_\-]{32,}"),
    ("aws access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    ("aws secret key", r"(?i)aws_secret_access_key\s*[=:]\s*\S{30,}"),
    ("npm token", r"\bnpm_[A-Za-z0-9]{30,}"),
    ("npm auth line", r"(?i)_authToken\s*=\s*\S{8,}"),
    ("huggingface token", r"\bhf_[A-Za-z0-9]{30,}"),
    ("gitlab token", r"\bglpat-[A-Za-z0-9_\-]{20,}"),
    ("google api key", r"\bAIza[0-9A-Za-z_\-]{35}"),
    ("slack token", r"\bxox[baprs]-[A-Za-z0-9\-]{10,}"),
    ("credentials in a url", r"[A-Za-z][A-Za-z0-9+.\-]*://[^/\s:@]+:[^/\s@]{3,}@"),
    ("netrc login", r"(?i)\bmachine\s+\S+\s+login\s+\S+\s+password\s+\S+"),
    ("docker auth", r#"(?i)"auth"\s*:\s*"[A-Za-z0-9+/=]{20,}""#),
    // short passwords: a quoted literal of 8+ characters assigned to a password-like name, or an exported one
    ("password literal", r#"(?i)\b[a-z0-9_]*(?:passw(?:or)?d|pwd|passphrase|secret)[a-z0-9_]*\s*[=:]\s*(?:"[^"\s$]{8,}"|'[^'\s$]{8,}')"#),
    ("exported secret", r#"(?i)^\s*export\s+[a-z0-9_]*(?:passw(?:or)?d|pwd|passphrase|secret|token|api_?key)[a-z0-9_]*=[^\s"'$(`][^\s]{7,}"#),
    ("secret assignment", r#"(?i)\b[a-z0-9_]*(?:api[_\-]?key|secret|token|passw(?:or)?d|credential)[a-z0-9_]*\s*[=:]\s*['"]?[A-Za-z0-9_\-/+=.]{16,}"#),
];

fn compiled() -> &'static Vec<(&'static str, Regex)> {
    static C: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    C.get_or_init(|| RULES.iter().map(|(n, p)| (*n, Regex::new(p).expect("rule compiles"))).collect())
}

/// Lines longer than this are scanned in pieces that overlap, so memory stays small whatever the file holds
/// (a 3 GB file of zeros has no line breaks at all).
const PIECE: usize = 1 << 20;
const OVERLAP: usize = 4096;

/// Every place in a stream that looks like a secret, one finding per line per rule, read piece by piece.
pub fn scan_reader(mut r: impl std::io::BufRead) -> std::io::Result<Vec<Finding>> {
    use std::io::{BufRead, Read};
    let (mut out, mut buf, mut line) = (Vec::<Finding>::new(), Vec::new(), 1usize);
    loop {
        let n = r.by_ref().take(PIECE as u64).read_until(b'\n', &mut buf)?;
        if n == 0 { break; }
        let ended = buf.last() == Some(&b'\n');
        if ended { buf.pop(); }
        for (rule, re) in compiled() {
            if re.is_match(&buf) && !out.iter().rev().take_while(|f| f.line == line).any(|f| f.rule == *rule) {
                out.push(Finding { rule, line });
            }
        }
        if ended { line += 1; buf.clear(); }
        else if n == PIECE { buf.drain(..buf.len().saturating_sub(OVERLAP)); }
    }
    Ok(out)
}

/// Every place in `data` that looks like a secret.
pub fn scan(data: &[u8]) -> Vec<Finding> {
    scan_reader(data).expect("reading memory cannot fail")
}

/// `path` if it is a file, or every file below it if it is a folder (never `.git`, never through a symlink).
pub fn files_under(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(path)?;
    if md.is_dir() {
        if path.file_name().is_some_and(|n| n == ".git") { return Ok(()); }
        for e in std::fs::read_dir(path)? { files_under(&e?.path(), out)?; }
    } else if md.is_file() {
        out.push(path.to_path_buf());
    }
    Ok(())
}

/// What to check in a git repository.
pub enum Scope<'a> { Staged, Range(&'a str), AllHistory }

/// Secrets in the lines a repository is about to add: (file, finding). Binary files are included.
pub fn scan_git(repo: &std::path::Path, scope: Scope) -> anyhow::Result<Vec<(String, Finding)>> {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(repo).args(["-c", "core.quotepath=off"]);
    match scope {
        Scope::Staged => { git.args(["diff", "--cached", "--text", "--no-color", "-U0"]); }
        Scope::AllHistory => { git.args(["log", "-p", "--text", "--no-color", "-U0", "--all"]); }
        Scope::Range(r) => { git.args(["log", "-p", "--text", "--no-color", "-U0", r, "--"]); }
    }
    let out = git.output()?;
    anyhow::ensure!(out.status.success(), "git failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    let (mut found, mut file, mut added) = (Vec::new(), String::from("?"), Vec::new());
    let flush = |file: &str, added: &mut Vec<u8>, found: &mut Vec<(String, Finding)>| {
        for f in scan(added) { found.push((file.to_string(), f)); }
        added.clear();
    };
    for line in out.stdout.split(|b| *b == b'\n') {
        if let Some(f) = line.strip_prefix(b"+++ b/") { flush(&file, &mut added, &mut found); file = String::from_utf8_lossy(f).into_owned(); }
        else if line.starts_with(b"+") && !line.starts_with(b"+++") { added.extend_from_slice(&line[1..]); added.push(b'\n'); }
    }
    flush(&file, &mut added, &mut found);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> { scan(text.as_bytes()).iter().map(|f| f.rule).collect() }
    // Fake values are assembled at runtime so this file never contains something that looks like a real token.
    fn fake(prefix: &str, n: usize) -> String { format!("{prefix}{}", &"a1B2c3D4e5".repeat(n / 10 + 1)[..n]) }

    #[test]
    fn modern_token_formats_are_caught() {
        for (text, want) in [
            (format!("export ANTHROPIC_API_KEY={}", fake("sk-ant-api03-", 40)), "anthropic key"),
            (format!("OPENAI_API_KEY={}", fake("sk-proj-", 48)), "openai-style key"),
            (format!("GITHUB_TOKEN={}", fake("gho_", 36)), "github token"),
            (format!("//registry.npmjs.org/:_authToken={}", fake("npm_", 36)), "npm token"),
            (format!("token = {}", fake("hf_", 34)), "huggingface token"),
            (fake("glpat-", 22), "gitlab token"),
            (format!("key={}", fake("AIza", 35)), "google api key"),
            (fake("AKIA", 16).to_uppercase(), "aws access key"),
            (format!("aws_secret_access_key = {}", fake("", 40)), "aws secret key"),
            ("machine example.com login me password hunter2hunter2".to_string(), "netrc login"),
            (format!("\"auth\": \"{}\"", fake("", 30)), "docker auth"),
            ("remote = https://user:hunter2hunter2@example.com/repo.git".to_string(), "credentials in a url"),
            (format!("-----BEGIN {} PRIVATE KEY-----", "OPENSSH"), "private key"),
        ] {
            assert!(rules(&text).contains(&want), "{want} not found in a line shaped like {}", want);
        }
    }

    #[test]
    fn case_does_not_matter() {
        assert!(!rules(&format!("api_key={}", fake("", 24))).is_empty());
        assert!(!rules(&format!("API_KEY={}", fake("", 24))).is_empty());
        assert!(!rules(&format!("My_Secret_Token: {}", fake("", 24))).is_empty());
    }

    #[test]
    fn binary_files_are_scanned() {
        let mut blob = vec![0u8, 159, 146, 150, 0, 255];
        blob.extend_from_slice(fake("ghp_", 40).as_bytes());
        blob.extend_from_slice(&[0, 1, 2]);
        assert_eq!(scan(&blob).len(), 1);
    }

    #[test]
    fn ordinary_config_is_clean() {
        for t in ["export PATH=$HOME/.cargo/bin:$PATH", "alias ll='ls -la'", "token=$TOKEN", "# password: ask the admin", "[user]\n  name = Someone", "font-name='Ubuntu 11'", "http://example.com/a/b"] {
            assert!(scan(t.as_bytes()).is_empty(), "false positive: {t}");
        }
    }

    #[test]
    fn a_huge_line_is_scanned_in_overlapping_pieces() {
        // a token that straddles a piece boundary is still found, once, on line 2
        let tok = fake("ghp_", 40);
        let mut data = b"ok\n".to_vec();
        data.extend(std::iter::repeat_n(b' ', PIECE - 20));
        data.extend_from_slice(tok.as_bytes());
        data.extend(std::iter::repeat_n(b' ', 100));
        let f = scan(&data);
        assert_eq!(f.iter().map(|x| (x.line, x.rule)).collect::<Vec<_>>(), vec![(2, "github token")]);
    }

    #[test]
    fn findings_name_the_line_not_the_secret() {
        let f = scan(format!("one\ntwo\nGITHUB_TOKEN={}", fake("gho_", 36)).as_bytes());
        assert_eq!(f.last().map(|x| x.line), Some(3));
    }
    #[test]
    fn short_passwords_are_caught_and_ordinary_code_is_not() {
        // built from parts so no source line holds a login-and-password shape
        let pw = format!("{}{}", "pass", "word");
        let hit = |line: String| !scan(line.as_bytes()).is_empty();
        assert!(hit(format!("DB_{pw}=\"hunter2hunter\"")), "quoted literal");
        assert!(hit(format!("{pw}: 'correcthorse'")), "yaml-style quoted");
        assert!(hit(format!("export MY_{}=abcd1234ef", pw.to_uppercase())), "exported, unquoted");
        assert!(hit("export API_KEY=abcdefgh1234".to_string()), "exported key");
        // things that look similar but are not secrets
        assert!(!hit(format!("{}_MIN_LENGTH=8", pw.to_uppercase())), "a number");
        assert!(!hit(format!("export {}_FILE=$HOME/.pw", pw.to_uppercase())), "a variable reference");
        assert!(!hit(format!("{pw} = input(\"Enter it: \")")), "a prompt");
        assert!(!hit(format!("{pw}: \"\"")), "empty");
        assert!(!hit(format!("# set your {pw} in the settings")), "a comment");
        assert!(!hit(format!("{pw} = \"$SOME_VARIABLE\"")), "a reference in quotes");
    }
}
