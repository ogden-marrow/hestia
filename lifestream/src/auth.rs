//! Sign in with GitHub. There is no Hestia account and no Hestia server: GitHub is only private storage for the
//! person's own home repository, and this module is how Hestia gets permission to use it.
//!
//! Safety rules, all enforced here:
//!  * The token lives ONLY in the system keyring (Secret Service). If there is no keyring, sign-in is refused
//!    (`no-keyring`). It is never written to a file, an environment variable, a command line, a log or a status line.
//!  * Every line this module prints is a JSON event; none ever contains the token. Tests assert that.
//!  * Calls go to GitHub over https and never follow a redirect, so a token cannot be sent somewhere else. The two
//!    base URLs can only be overridden to a loopback address (for tests).
//!  * Only the scope `repo` is requested.
//!
//! Output contract (one JSON object per line): see issue #110.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::time::{Duration, Instant};

/// The public Client ID of Hestia's GitHub OAuth App (device flow). It is not a secret. Set at build time with
/// `HESTIA_GITHUB_CLIENT_ID`; without it sign-in says it is not set up in this build.
pub const CLIENT_ID: &str = match option_env!("HESTIA_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
const SCOPE: &str = "repo";
const SERVICE: &str = "app.hestia.Hestia";
const ACCOUNT: &str = "github";

// ---- where the token lives -------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum StoreErr {
    /// No system keyring is available. Hestia refuses to keep a token anywhere else.
    NoKeyring,
    Other(String),
}

pub trait Store {
    fn get(&self) -> Result<Option<String>, StoreErr>;
    fn set(&self, token: &str) -> Result<(), StoreErr>;
    fn delete(&self) -> Result<(), StoreErr>;
}

/// The system keyring (GNOME Keyring / KWallet through the Secret Service). Encrypted at rest by the desktop.
pub struct Keyring;

fn map_kr(e: keyring::Error) -> StoreErr {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => StoreErr::NoKeyring,
        other => StoreErr::Other(other.to_string()),
    }
}

impl Keyring {
    fn entry() -> Result<keyring::Entry, StoreErr> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(map_kr)
    }
}

impl Store for Keyring {
    fn get(&self) -> Result<Option<String>, StoreErr> {
        match Self::entry()?.get_password() {
            Ok(t) => Ok(Some(t)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_kr(e)),
        }
    }
    fn set(&self, token: &str) -> Result<(), StoreErr> {
        Self::entry()?.set_password(token).map_err(map_kr)
    }
    fn delete(&self) -> Result<(), StoreErr> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_kr(e)),
        }
    }
}

// ---- where GitHub is -------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Endpoints {
    pub web: String,
    pub api: String,
}

impl Endpoints {
    pub fn github() -> Self {
        Self { web: "https://github.com".into(), api: "https://api.github.com".into() }
    }
    /// The real GitHub, unless a test points both at a loopback fake. Anything else is refused.
    pub fn from_env() -> Result<Self> {
        let mut e = Self::github();
        for (var, slot) in [("HESTIA_GITHUB_WEB", &mut e.web), ("HESTIA_GITHUB_API", &mut e.api)] {
            if let Ok(v) = std::env::var(var) {
                crate::semantic::ensure_local(&v).map_err(|_| anyhow::anyhow!("{var} may only point at this computer"))?;
                *slot = v.trim_end_matches('/').to_string();
            }
        }
        Ok(e)
    }
}

pub fn agent() -> ureq::Agent {
    // no redirects: a token is only ever sent to the address we chose
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).redirects(0).user_agent("Hestia").build()
}

// ---- events ----------------------------------------------------------------------------------------------------

pub fn emit(out: &mut dyn Write, v: Value) {
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// `kind` is one of: offline denied expired sso exists no-keyring other.
pub fn failed(out: &mut dyn Write, kind: &str, extra: Option<(&str, &str)>) {
    let mut v = json!({"event": "failed", "kind": kind});
    if let Some((k, val)) = extra {
        v[k] = json!(val);
    }
    emit(out, v);
}

/// How an HTTP failure reads to a person.
pub fn classify(e: ureq::Error) -> (&'static str, Option<(&'static str, String)>) {
    match e {
        ureq::Error::Transport(_) => ("offline", None),
        ureq::Error::Status(403, r) if r.header("x-github-sso").is_some() => ("sso", Some(("org", String::new()))),
        ureq::Error::Status(c, _) => ("other", Some(("message", format!("GitHub answered {c}")))),
    }
}

fn fail_http(out: &mut dyn Write, e: ureq::Error) {
    let (kind, extra) = classify(e);
    failed(out, kind, extra.as_ref().map(|(k, v)| (*k, v.as_str())));
}

fn fail_store(out: &mut dyn Write, e: StoreErr) {
    match e {
        StoreErr::NoKeyring => failed(out, "no-keyring", None),
        StoreErr::Other(m) => failed(out, "other", Some(("message", &format!("the keyring said: {m}")))),
    }
}

// ---- commands --------------------------------------------------------------------------------------------------

/// Who is this token? `None` means GitHub does not accept it any more.
pub fn whoami(ep: &Endpoints, token: &str) -> Result<Option<String>, ureq::Error> {
    match agent().get(&format!("{}/user", ep.api)).set("Authorization", &format!("Bearer {token}")).set("Accept", "application/vnd.github+json").call() {
        Ok(r) => Ok(r.into_json::<Value>().ok().and_then(|v| v["login"].as_str().map(String::from))),
        Err(ureq::Error::Status(401, _)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Start the device flow, show the code, wait for the person to approve in their browser, keep the token in the keyring.
pub fn login(out: &mut dyn Write, store: &dyn Store, ep: &Endpoints, client_id: &str) -> bool {
    if client_id.is_empty() {
        failed(out, "other", Some(("message", "sign-in is not set up in this build")));
        return false;
    }
    // refuse before showing a code if there is nowhere safe to put the token
    if let Err(e) = store.get() {
        fail_store(out, e);
        return false;
    }
    let a = agent();
    let start: Value = match a.post(&format!("{}/login/device/code", ep.web)).set("Accept", "application/json").send_form(&[("client_id", client_id), ("scope", SCOPE)]) {
        Ok(r) => r.into_json().unwrap_or(Value::Null),
        Err(e) => {
            fail_http(out, e);
            return false;
        }
    };
    let (Some(device), Some(user_code), Some(uri)) = (start["device_code"].as_str(), start["user_code"].as_str(), start["verification_uri"].as_str()) else {
        failed(out, "other", Some(("message", "GitHub did not return a sign-in code")));
        return false;
    };
    let expires = start["expires_in"].as_u64().unwrap_or(900);
    let mut interval = start["interval"].as_u64().unwrap_or(5);
    emit(out, json!({"event": "code", "user_code": user_code, "verification_uri": uri, "expires_in": expires, "scope": SCOPE}));

    let deadline = Instant::now() + Duration::from_secs(expires);
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        if Instant::now() >= deadline {
            failed(out, "expired", None);
            return false;
        }
        let r: Value = match a
            .post(&format!("{}/login/oauth/access_token", ep.web))
            .set("Accept", "application/json")
            .send_form(&[("client_id", client_id), ("device_code", device), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")])
        {
            Ok(r) => r.into_json().unwrap_or(Value::Null),
            Err(e) => {
                fail_http(out, e);
                return false;
            }
        };
        if let Some(token) = r["access_token"].as_str() {
            return finish(out, store, ep, token);
        }
        match r["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval = r["interval"].as_u64().unwrap_or(interval + 5),
            Some("expired_token") => {
                failed(out, "expired", None);
                return false;
            }
            Some("access_denied") => {
                failed(out, "denied", None);
                return false;
            }
            _ => {
                failed(out, "other", Some(("message", "GitHub did not accept the sign-in")));
                return false;
            }
        }
    }
}

fn finish(out: &mut dyn Write, store: &dyn Store, ep: &Endpoints, token: &str) -> bool {
    let user = match whoami(ep, token) {
        Ok(Some(u)) => u,
        Ok(None) => {
            failed(out, "denied", None);
            return false;
        }
        Err(e) => {
            fail_http(out, e);
            return false;
        }
    };
    if let Err(e) = store.set(token) {
        fail_store(out, e);
        return false;
    }
    emit(out, json!({"event": "approved", "user": user}));
    true
}

pub fn status(out: &mut dyn Write, store: &dyn Store, ep: &Endpoints) -> bool {
    let token = match store.get() {
        Ok(Some(t)) => t,
        Ok(None) => {
            emit(out, json!({"event": "status", "signed_in": false}));
            return true;
        }
        Err(e) => {
            fail_store(out, e);
            return false;
        }
    };
    match whoami(ep, &token) {
        Ok(Some(user)) => emit(out, json!({"event": "status", "signed_in": true, "user": user})),
        Ok(None) => emit(out, json!({"event": "status", "signed_in": false})),
        Err(e) => {
            fail_http(out, e);
            return false;
        }
    }
    true
}

/// Forget the token on this computer. (GitHub keeps the authorisation until it is removed at
/// github.com/settings/applications: a device-flow app has no secret with which to revoke it.)
pub fn logout(out: &mut dyn Write, store: &dyn Store) -> bool {
    match store.delete() {
        Ok(()) => {
            emit(out, json!({"event": "signed_out"}));
            true
        }
        Err(e) => {
            fail_store(out, e);
            false
        }
    }
}

/// `git` asks for a password: answer from the keyring, only for github.com over https. This is how pushes and
/// clones work without the token ever being written to disk. Protocol: key=value lines on stdin, blank line ends.
pub fn git_credential(op: &str, input: &str, store: &dyn Store, out: &mut dyn Write) -> Result<()> {
    if op != "get" {
        return Ok(()); // store and erase: nothing to do, we never keep what git hands us
    }
    let mut host = "";
    let mut proto = "";
    for line in input.lines() {
        match line.split_once('=') {
            Some(("host", v)) => host = v,
            Some(("protocol", v)) => proto = v,
            _ => {}
        }
    }
    if host != "github.com" || proto != "https" {
        return Ok(());
    }
    match store.get() {
        Ok(Some(t)) => {
            write!(out, "username=x-access-token\npassword={t}\n")?;
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(_) => bail!("the system keyring is not available"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const TOKEN: &str = "gho_FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0123";

    struct Mem(RefCell<Option<String>>, bool);
    impl Store for Mem {
        fn get(&self) -> Result<Option<String>, StoreErr> {
            if self.1 { Err(StoreErr::NoKeyring) } else { Ok(self.0.borrow().clone()) }
        }
        fn set(&self, t: &str) -> Result<(), StoreErr> {
            if self.1 { return Err(StoreErr::NoKeyring) }
            *self.0.borrow_mut() = Some(t.into());
            Ok(())
        }
        fn delete(&self) -> Result<(), StoreErr> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }
    fn mem() -> Mem { Mem(RefCell::new(None), false) }

    /// A GitHub that answers each path from a script, one reply per call, in order. Records nothing about bodies.
    fn fake(script: Vec<(&'static str, u16, &'static str)>) -> (Endpoints, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let script = Arc::new(Mutex::new(script));
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                // read the whole request: headers, then Content-Length bytes of body
                let mut data: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 { break }
                    data.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&data).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let want = text[..h].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                        if data.len() >= h + 4 + want { break }
                    }
                }
                let req = String::from_utf8_lossy(&data).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                seen2.lock().unwrap().push(req.clone());
                let mut sc = script.lock().unwrap();
                let pos = sc.iter().position(|(p, _, _)| *p == path);
                let (code, body) = match pos { Some(i) => { let (_, c, b) = sc.remove(i); (c, b) } None => (404, "{}") };
                let sso = if code == 403 { "X-GitHub-SSO: required; url=https://x\r\n" } else { "" };
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\n{sso}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (Endpoints { web: base.clone(), api: base }, seen)
    }

    const CODE: &str = r#"{"device_code":"dc","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":0}"#;
    const TOKEN_REPLY: &str = r#"{"access_token":"gho_FAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE0123","token_type":"bearer","scope":"repo"}"#;

    fn run_login(script: Vec<(&'static str, u16, &'static str)>, store: &Mem) -> (bool, String, Arc<Mutex<Vec<String>>>) {
        let (ep, seen) = fake(script);
        let mut out = Vec::new();
        let ok = login(&mut out, store, &ep, "client123");
        (ok, String::from_utf8(out).unwrap(), seen)
    }

    #[test]
    fn signs_in_stores_the_token_and_never_prints_it() {
        let store = mem();
        let (ok, out, _) = run_login(vec![
            ("/login/device/code", 200, CODE),
            ("/login/oauth/access_token", 200, r#"{"error":"authorization_pending"}"#),
            ("/login/oauth/access_token", 200, r#"{"error":"slow_down","interval":0}"#),
            ("/login/oauth/access_token", 200, TOKEN_REPLY),
            ("/user", 200, r#"{"login":"someone"}"#),
        ], &store);
        assert!(ok, "{out}");
        assert!(out.lines().next().unwrap().contains(r#""event":"code""#) && out.contains("ABCD-1234") && out.contains(r#""scope":"repo""#));
        assert!(out.contains(r#""event":"approved""#) && out.contains("someone"));
        assert_eq!(store.get().unwrap().as_deref(), Some(TOKEN));
        assert!(!out.contains(TOKEN) && !out.contains("gho_"), "the token must never be printed: {out}");
    }

    #[test]
    fn refuses_before_showing_a_code_when_there_is_no_keyring() {
        let store = Mem(RefCell::new(None), true);
        let (ok, out, seen) = run_login(vec![("/login/device/code", 200, CODE)], &store);
        assert!(!ok);
        assert!(out.contains(r#""kind":"no-keyring""#) && !out.contains("ABCD"));
        assert!(seen.lock().unwrap().is_empty(), "must not even contact GitHub");
    }

    #[test]
    fn denied_and_expired_are_reported_and_nothing_is_stored() {
        for (reply, kind) in [(r#"{"error":"access_denied"}"#, "denied"), (r#"{"error":"expired_token"}"#, "expired")] {
            let store = mem();
            let (ok, out, _) = run_login(vec![("/login/device/code", 200, CODE), ("/login/oauth/access_token", 200, reply)], &store);
            assert!(!ok && out.contains(&format!(r#""kind":"{kind}""#)), "{out}");
            assert!(store.get().unwrap().is_none());
        }
    }

    #[test]
    fn a_token_github_rejects_is_not_stored() {
        let store = mem();
        let (ok, out, _) = run_login(vec![("/login/device/code", 200, CODE), ("/login/oauth/access_token", 200, TOKEN_REPLY), ("/user", 401, "{}")], &store);
        assert!(!ok && store.get().unwrap().is_none(), "{out}");
    }

    #[test]
    fn sso_and_offline_are_named() {
        let store = mem();
        let (ok, out, _) = run_login(vec![("/login/device/code", 200, CODE), ("/login/oauth/access_token", 200, TOKEN_REPLY), ("/user", 403, "{}")], &store);
        assert!(!ok && out.contains(r#""kind":"sso""#), "{out}");
        let dead = Endpoints { web: "http://127.0.0.1:9".into(), api: "http://127.0.0.1:9".into() };
        let mut o = Vec::new();
        assert!(!login(&mut o, &mem(), &dead, "client123"));
        assert!(String::from_utf8(o).unwrap().contains(r#""kind":"offline""#));
    }

    #[test]
    fn an_unconfigured_build_says_so() {
        let mut o = Vec::new();
        assert!(!login(&mut o, &mem(), &Endpoints::github(), ""));
        assert!(String::from_utf8(o).unwrap().contains("not set up in this build"));
    }

    #[test]
    fn status_and_logout() {
        let store = mem();
        let mut o = Vec::new();
        status(&mut o, &store, &Endpoints::github());
        assert!(String::from_utf8(o).unwrap().contains(r#""signed_in":false"#));
        store.set(TOKEN).unwrap();
        let (ep, _) = fake(vec![("/user", 200, r#"{"login":"someone"}"#)]);
        let mut o = Vec::new();
        assert!(status(&mut o, &store, &ep));
        let s = String::from_utf8(o).unwrap();
        assert!(s.contains(r#""signed_in":true"#) && s.contains("someone") && !s.contains(TOKEN));
        let (ep, _) = fake(vec![("/user", 401, "{}")]);
        let mut o = Vec::new();
        status(&mut o, &store, &ep);
        assert!(String::from_utf8(o).unwrap().contains(r#""signed_in":false"#), "a rejected token is signed out");
        let mut o = Vec::new();
        assert!(logout(&mut o, &store));
        assert!(store.get().unwrap().is_none() && String::from_utf8(o).unwrap().contains("signed_out"));
    }

    #[test]
    fn git_gets_the_token_only_for_github_over_https() {
        let store = mem();
        store.set(TOKEN).unwrap();
        let ask = |op: &str, input: &str| { let mut o = Vec::new(); git_credential(op, input, &store, &mut o).unwrap(); String::from_utf8(o).unwrap() };
        assert!(ask("get", "protocol=https\nhost=github.com\n\n").contains("password=gho_"));
        assert_eq!(ask("get", "protocol=https\nhost=evil.example\n\n"), "");
        assert_eq!(ask("get", "protocol=http\nhost=github.com\n\n"), "");
        assert_eq!(ask("get", "protocol=https\nhost=github.com.evil.example\n\n"), "");
        assert_eq!(ask("store", "protocol=https\nhost=github.com\npassword=x\n\n"), "");
    }

    #[test]
    fn endpoints_can_only_be_overridden_to_this_computer() {
        std::env::set_var("HESTIA_GITHUB_API", "https://evil.example");
        assert!(Endpoints::from_env().is_err());
        std::env::set_var("HESTIA_GITHUB_API", "http://127.0.0.1:1234/");
        assert_eq!(Endpoints::from_env().unwrap().api, "http://127.0.0.1:1234");
        std::env::remove_var("HESTIA_GITHUB_API");
    }
}
