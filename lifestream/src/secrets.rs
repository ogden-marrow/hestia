//! One secret scanner, used everywhere something could leave the machine: capture, push, publish.
//! It reports which rule matched and where, never the secret itself. It reads bytes, so binary files
//! (token databases) are scanned too. Case does not matter.

use regex::bytes::Regex;
use std::sync::OnceLock;

pub struct Finding {
    pub rule: &'static str,
    pub line: usize,
}

// The name of something secret, and a value that looks like one: 8+ characters, not a variable reference ($, ${}),
// a placeholder (<...>, {{...}}), a path or a call. Shared by the line-start and the inline rules below.
macro_rules! secret_name { () => { r"(?:[a-z0-9_\-]*(?:passw(?:or)?d|pwd|passphrase|secret|token|api[_\-]?key)[a-z0-9_\-]*|(?:[a-z0-9]+[_\-])*pass)" } }
macro_rules! secret_value { () => { r#"[^\s"'$(){}<>\[\]\\`/~.][^\s"'$(){}<>\[\]\\`]{7,}(?:[\s;,&"')\]]|$)"# } }

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
    ("slack token", r"\bxox[a-z]-[A-Za-z0-9%_\-]{10,}"),
    ("slack webhook", r"hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9/_\-]{16,}"),
    ("stripe live key", r"\b[sr]k_live_[A-Za-z0-9]{16,}"),
    ("sendgrid key", r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}"),
    ("json web token", r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]*"),
    ("authorization header", r#"(?i)\bauthorization\b["']?\s*[:=]\s*["']?(?:bearer|basic|token)\s+[A-Za-z0-9._~+/=\-]{12,}"#),
    ("azure storage key", r"(?i)\b(?:account|sharedaccess)key\s*=\s*[A-Za-z0-9+/]{30,}"),
    ("wireguard key", r"(?i)\b(?:private|preshared)key\s*=\s*[A-Za-z0-9+/]{43}="),
    ("kubernetes client key", r"(?i)\bclient-key-data\s*:\s*[A-Za-z0-9+/=]{40,}"),
    // a PEM private key wrapped in base64 (kubeconfig, json, env files): "-----BEGIN ... PRIVATE KEY" at the three possible alignments
    ("private key (base64)", r"LS0tLS1CRUdJTi[A-Za-z0-9+/]{0,24}(?:UFJJVkFURSBLRVk|QUklWQVRFIEtFW|IFBSSVZBVEUgS0V)"),
    ("credentials in a url", r"[A-Za-z][A-Za-z0-9+.\-]*://[^/\s:@]+:[^/\s@]{3,}@"),
    ("netrc login", r"(?i)\bmachine\s+\S+\s+login\s+\S+\s+password\s+\S+"),
    ("docker auth", r#"(?i)"auth"\s*:\s*"[A-Za-z0-9+/=]{20,}""#),
    // short passwords: a quoted literal of 8+ characters assigned to a password-like name, or an exported one
    ("password literal", r#"(?i)\b[a-z0-9_]*(?:passw(?:or)?d|pwd|passphrase|secret)[a-z0-9_]*["']?\s*[=:]\s*(?:"[^"\s$]{8,}"|'[^'\s$]{8,}')"#),
    // PASSWORD=..., export DB_PASS=..., password = ... in an ini file, rclone's `pass = ...`
    ("secret setting", concat!(r"(?i)^\s*(?:export\s+)?", secret_name!(), r"\s*=\s*", secret_value!())),
    // inline: host=x password=..., MYSQL_PWD=..., ?token=..., --password=..., --api-key ...
    ("secret argument", concat!(r#"(?i)(?:^|[\s;&?"'])(?:--)?"#, secret_name!(), "=", secret_value!())),
    ("secret flag", concat!(r"(?i)(?:^|\s)--", secret_name!(), r"\s+", secret_value!())),
    ("curl credentials", r#"(?i)\bcurl\b.*\s(?:-u|--user)(?:\s+|=)['"]?[^\s:'"$]+:[^\s'"$]{3,}"#),
    ("secret assignment", r#"(?i)\b[a-z0-9_]*(?:api[_\-]?key|secret|token|passw(?:or)?d|credential)[a-z0-9_]*["']?\s*[=:]\s*['"]?[A-Za-z0-9_\-/+=.]{16,}"#),
];

fn compiled() -> &'static Vec<(&'static str, Regex)> {
    static C: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    C.get_or_init(|| RULES.iter().map(|(n, p)| (*n, Regex::new(p).expect("rule compiles"))).collect())
}

/// Every place in `data` that looks like a secret, one finding per line per rule.
pub fn scan(data: &[u8]) -> Vec<Finding> {
    let mut out = Vec::new();
    for (i, line) in data.split(|b| *b == b'\n').enumerate() {
        for (rule, re) in compiled() {
            if re.is_match(line) {
                out.push(Finding { rule, line: i + 1 });
            }
        }
    }
    out
}

/// What to check in a git repository.
pub enum Scope<'a> { Staged, Range(&'a str), AllHistory }

/// How many lines a hunk header ("@@ -a,b +c,d @@") says the hunk adds.
fn added_in(header: &[u8]) -> usize {
    String::from_utf8_lossy(header).split_whitespace().find(|t| t.starts_with('+'))
        .map(|t| t[1..].split(',').nth(1).and_then(|n| n.parse().ok()).unwrap_or(1)).unwrap_or(0)
}

/// Secrets in the lines a repository is about to add: (file, finding). Binary files are included. A merge commit is
/// shown against its first parent, so what only the merge brings in (a conflict resolution) is scanned too.
pub fn scan_git(repo: &std::path::Path, scope: Scope) -> anyhow::Result<Vec<(String, Finding)>> {
    let mut git = std::process::Command::new("git");
    git.arg("-C").arg(repo).args(["-c", "core.quotepath=off"]);
    let flags = ["--text", "--no-textconv", "--no-ext-diff", "--no-color", "-U0", "--src-prefix=a/", "--dst-prefix=b/"];
    match scope {
        Scope::Staged => { git.args(["diff", "--cached"]).args(flags); }
        Scope::AllHistory => { git.args(["log", "-p", "--diff-merges=first-parent", "--all"]).args(flags); }
        Scope::Range(r) => { git.args(["log", "-p", "--diff-merges=first-parent"]).args(flags).args([r, "--"]); }
    }
    let out = git.output()?;
    anyhow::ensure!(out.status.success(), "git failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    let (mut found, mut file, mut added, mut left) = (Vec::new(), String::from("?"), Vec::new(), 0usize);
    let flush = |file: &str, added: &mut Vec<u8>, found: &mut Vec<(String, Finding)>| {
        for f in scan(added) { found.push((file.to_string(), f)); }
        added.clear();
    };
    // The hunk header says how many lines it adds, so an added line that itself starts with "++" is never mistaken for a file header.
    for line in out.stdout.split(|b| *b == b'\n') {
        if left > 0 && line.starts_with(b"+") { added.extend_from_slice(&line[1..]); added.push(b'\n'); left -= 1; }
        else if let Some(h) = line.strip_prefix(b"@@ ") { left = added_in(h); }
        else if let Some(f) = line.strip_prefix(b"+++ b/") { flush(&file, &mut added, &mut found); file = String::from_utf8_lossy(f).into_owned(); }
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

    // --- red-team round 3 (#175): formats and shapes the first rules missed ---

    #[test]
    fn json_keys_and_bare_values_are_caught() {
        let pw = format!("{}{}", "pass", "word");
        let hit = |line: String| !scan(line.as_bytes()).is_empty();
        let v = fake("", 20);
        assert!(hit(format!("{{\"access_token\":\"{v}\"}}")), "json access_token, closing quote before the colon");
        assert!(hit(format!("  \"refresh_token\": \"{v}\",")), "json refresh_token");
        assert!(hit(format!("{{\"apiKey\": \"{v}\"}}")), "json apiKey");
        assert!(hit(format!("{{\"{pw}\":\"hunter2hunter\"}}")), "json short password");
        assert!(hit(format!("{}=hunter2hunter", pw.to_uppercase())), "PASSWORD=value, no export");
        assert!(hit("DB_PASS=hunter2hunter".to_string()), "DB_PASS");
        assert!(hit("MYSQL_PWD=hunter2hunter".to_string()), "MYSQL_PWD");
        assert!(hit(format!("{pw} = hunter2hunter")), "ini style with spaces");
        assert!(hit("pass = hunter2hunter".to_string()), "rclone pass");
        assert!(hit(format!("host=db.example dbname=app user=app {pw}=hunter2hunter")), "libpq connection string");
        assert!(hit(format!("mysql --{pw}=hunter2hunter -h db")), "--password=value");
        assert!(hit(format!("tool --api-key {}", fake("", 20))), "--api-key value");
        assert!(hit(format!("curl -u {}:{} https://x.example/api", "me", "hunter2hunter")), "curl -u");
        assert!(hit(format!("curl --user=me:{} https://x.example/api", "hunter2hunter")), "curl --user=");
    }

    #[test]
    fn vendor_tokens_are_caught() {
        let b64 = |n: usize| fake("", n);
        for (text, want) in [
            (fake("xoxc-", 40), "slack token"),
            (fake("xoxd-", 40), "slack token"),
            (format!("https://hooks.{}.com/services/T0{}", "slack", b64(30)), "slack webhook"),
            (fake("sk_live_", 30), "stripe live key"),
            (fake("rk_live_", 30), "stripe live key"),
            (format!("{}.{}.{}", "SG", fake("", 22), fake("", 43)), "sendgrid key"),
            (format!("{}.{}.{}", fake("eyJ", 20), fake("eyJ", 30), b64(24)), "json web token"),
            (format!("Authorization: Bearer {}", b64(30)), "authorization header"),
            (format!("curl -H 'Authorization: Basic {}'", b64(16)), "authorization header"),
            (format!("\"authorization\": \"Bearer {}\"", b64(30)), "authorization header"),
            (format!("DefaultEndpointsProtocol=https;AccountName=x;AccountKey={}==", b64(60)), "azure storage key"),
            (format!("PrivateKey = {}=", b64(43)), "wireguard key"),
            (format!("    client-key-data: {}", b64(60)), "kubernetes client key"),
            (format!("key: {}{}{}", "LS0tLS1CRUdJTi", "BSU0EgUFJJVkFURSBLRVktLS0tLQ", b64(40)), "private key (base64)"),
            (format!("key: {}{}{}", "LS0tLS1CRUdJTi", "BQUklWQVRFIEtFWS0tLS0t", b64(40)), "private key (base64)"),
            (format!("key: {}{}{}", "LS0tLS1CRUdJTi", "BPUEVOU1NIIFBSSVZBVEUgS0VZ", b64(40)), "private key (base64)"),
        ] {
            assert!(rules(&text).contains(&want), "{want} not found in a line shaped like it");
        }
    }

    #[test]
    fn lookalikes_and_references_are_not_secrets() {
        let pw = format!("{}{}", "pass", "word");
        let up = pw.to_uppercase();
        for t in [
            format!("{up}=$DB_VALUE"), format!("{up}=${{DB_VALUE}}"), format!("{up}=<your-value-here>"), format!("{pw} = {{{{ .Values.db }}}}"),
            format!("{up}=short"), format!("{up}="), format!("{pw}={{}}"),
            format!("const token=getToken(request)"), format!("{pw}=request.form['x']"), format!("{pw} = os.environ.get(\"X\")"),
            "MAX_TOKENS=1".to_string(), "OLDPWD=/home/someone/projects".to_string(), "PWD=/home/someone".to_string(), "bypass=localhost.local".to_string(),
            "docker login --password-stdin".to_string(), format!("docker login --{pw}=$REGISTRY_VALUE"), "tool --token-file /run/secrets/t".to_string(),
            "tool --pass-through verbose-output".to_string(), format!("export {up}_FILE=/run/secrets/db"),
            "Authorization: Bearer $TOKEN".to_string(), "Authorization: Bearer <token>".to_string(), "Authorization: Bearer {{ token }}".to_string(),
            "curl -u $USER:$PASS https://x.example".to_string(), "curl -u me https://x.example".to_string(), "curl -s https://x.example/a?b=c".to_string(),
            "{\"token\": \"\", \"token_type\": \"bearer\"}".to_string(), format!("{{\"{pw}\": \"$DB_VALUE\"}}"),
            "see https://hooks.slack.com/services/ for webhooks".to_string(), "sk_live_ and SG. are prefixes".to_string(),
            "PrivateKey = <key>".to_string(), "# PrivateKey goes here".to_string(), "client-key-data: REDACTED".to_string(), "AccountKey=<key>".to_string(),
            format!("certificate-authority-data: {}{}{}", "LS0tLS1CRUdJTi", "BDRVJUSUZJQ0FURS0tLS0t", fake("", 40)), // a public certificate, not a key
            "eyJhbGciOi is how a JWT starts".to_string(),
        ] {
            assert!(scan(t.as_bytes()).is_empty(), "false positive: {t}");
        }
    }

    fn repo(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ls-secrets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        git(&d, &["init", "-q", "-b", "main"]);
        d
    }
    fn git(dir: &std::path::Path, a: &[&str]) -> String {
        let o = std::process::Command::new("git").arg("-C").arg(dir).args(["-c", "commit.gpgsign=false"]).args(a)
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t.t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t.t").output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    #[test]
    fn an_added_line_that_starts_with_plus_plus_is_still_scanned() {
        let d = repo("plusplus");
        // the added line is "++<secret>", which a diff shows as "+++<secret>": it must not be mistaken for a file header
        std::fs::write(d.join("a.txt"), format!("ok\n++GITHUB_TOKEN={}\n", fake("gho_", 36))).unwrap();
        git(&d, &["add", "."]);
        assert!(!scan_git(&d, Scope::Staged).unwrap().is_empty(), "staged");
        git(&d, &["commit", "-qm", "x"]);
        assert!(!scan_git(&d, Scope::AllHistory).unwrap().is_empty(), "history");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_secret_that_only_a_merge_brings_in_is_found() {
        let d = repo("merge");
        std::fs::write(d.join("f.txt"), "a\nb\nc\n").unwrap();
        git(&d, &["add", "."]); git(&d, &["commit", "-qm", "base"]);
        let base = git(&d, &["rev-parse", "HEAD"]);
        git(&d, &["checkout", "-qb", "side"]);
        std::fs::write(d.join("f.txt"), "a\nside\nc\n").unwrap();
        git(&d, &["commit", "-qam", "side"]);
        git(&d, &["checkout", "-q", "main"]);
        std::fs::write(d.join("f.txt"), "a\nmain\nc\n").unwrap();
        git(&d, &["commit", "-qam", "main"]);
        git(&d, &["merge", "side"]); // conflicts: resolved by hand below, and the resolution carries a secret
        std::fs::write(d.join("f.txt"), format!("a\nGITHUB_TOKEN={}\nc\n", fake("gho_", 36))).unwrap();
        git(&d, &["commit", "-qam", "merge"]);
        let range = format!("{base}..main");
        let found = scan_git(&d, Scope::Range(&range)).unwrap();
        assert!(!found.is_empty() && found.iter().all(|f| f.0 == "f.txt"), "the secret exists only in the merge commit");
        assert!(!scan_git(&d, Scope::AllHistory).unwrap().is_empty());
        // what the first parent already had is not "added" by the merge
        let none = scan_git(&d, Scope::Range(&format!("main..main"))).unwrap();
        assert!(none.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
