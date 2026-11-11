//! Phase 1b: find by meaning. Embeds each record's *metadata* (path, kind, tags,
//! never file contents) with a local model and ranks by cosine similarity.
//! Fails closed: refuses any host that is not loopback, so file names can
//! never leave this machine through this module.

use crate::{human, Record};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_HOST: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "nomic-embed-text";
const BATCH: usize = 64;

pub struct Row {
    pub id: String,
    pub text_hash: String,
    pub vec: Vec<f32>,
}

/// File names must never leave this machine: the model host has to be this computer, spelled plainly.
/// Strict on purpose: no user info (`http://127.0.0.1@evil.com`), no odd characters, a real loopback address.
pub fn ensure_local(host: &str) -> Result<()> {
    let refuse = || bail!("refusing non-local model host '{host}': file names must not leave this machine");
    let Some(rest) = host.strip_prefix("http://").or_else(|| host.strip_prefix("https://")) else { return refuse() };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains(['@', '\\', ' ', '%']) || !authority.is_ascii() {
        return refuse();
    }
    let (name, port) = if let Some(v6) = authority.strip_prefix('[') {
        let Some((ip, after)) = v6.split_once(']') else { return refuse() };
        (ip, after.strip_prefix(':').or(if after.is_empty() { Some("") } else { None }))
    } else {
        match authority.split_once(':') { Some((n, p)) => (n, Some(p)), None => (authority, Some("")) }
    };
    if !port.is_some_and(|p| p.chars().all(|c| c.is_ascii_digit())) {
        return refuse();
    }
    let local = name.eq_ignore_ascii_case("localhost")
        || name.parse::<std::net::Ipv4Addr>().is_ok_and(|a| a.is_loopback())
        || name.parse::<std::net::Ipv6Addr>().is_ok_and(|a| a.is_loopback());
    if local { Ok(()) } else { refuse() }
}

/// A client for the local model server: it never follows a redirect, so a server on this machine cannot send file names elsewhere.
pub fn local_agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(timeout).redirects(0).build()
}

fn store_paths(dir: &Path, model: &str) -> (PathBuf, PathBuf) {
    let m: String = model.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    (dir.join(format!("emb-{m}.ids")), dir.join(format!("emb-{m}.f32")))
}

pub fn load(dir: &Path, model: &str) -> Result<Vec<Row>> {
    let (ids_p, f32_p) = store_paths(dir, model);
    if !ids_p.exists() {
        return Ok(vec![]);
    }
    let ids = fs::read_to_string(&ids_p)?;
    let lines: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();
    let bytes = fs::read(&f32_p).with_context(|| format!("reading {}", f32_p.display()))?;
    if lines.is_empty() {
        return Ok(vec![]);
    }
    if bytes.len() % (4 * lines.len()) != 0 {
        bail!("{} and {} are out of sync; delete both and re-run `embed`", ids_p.display(), f32_p.display());
    }
    let dim = bytes.len() / 4 / lines.len();
    let mut rows = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let (id, h) = l.split_once('\t').context("bad ids line")?;
        let v: Vec<f32> = bytes[i * dim * 4..(i + 1) * dim * 4]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        rows.push(Row { id: id.into(), text_hash: h.into(), vec: v });
    }
    Ok(rows)
}

fn save(dir: &Path, model: &str, rows: &[Row]) -> Result<()> {
    fs::create_dir_all(dir)?;
    let (ids_p, f32_p) = store_paths(dir, model);
    let mut ids = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    for r in rows {
        ids.push_str(&format!("{}\t{}\n", r.id, r.text_hash));
        for x in &r.vec {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    // write the vectors first, then the index, via temp files
    let t1 = f32_p.with_extension("f32.tmp");
    let t2 = ids_p.with_extension("ids.tmp");
    fs::write(&t1, bytes)?;
    fs::write(&t2, ids)?;
    fs::rename(&t1, &f32_p)?;
    fs::rename(&t2, &ids_p)?;
    Ok(())
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

fn embed_batch(host: &str, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
    ensure_local(host)?;
    let agent = local_agent(Duration::from_secs(300));
    let mut last = None;
    for _ in 0..2 {
        let resp = agent
            .post(&format!("{}/api/embed", host.trim_end_matches('/')))
            .send_json(serde_json::json!({ "model": model, "input": inputs }));
        match resp {
            Ok(r) => {
                let v: serde_json::Value = r.into_json()?;
                let arr = v["embeddings"].as_array().context("no 'embeddings' in response")?;
                if arr.len() != inputs.len() {
                    bail!("asked for {} embeddings, got {}", inputs.len(), arr.len());
                }
                return arr
                    .iter()
                    .map(|e| {
                        Ok(normalize(
                            e.as_array().context("bad embedding")?.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect(),
                        ))
                    })
                    .collect();
            }
            Err(e) => last = Some(e),
        }
    }
    bail!("embedding request failed (is `ollama serve` running and `{model}` pulled?): {}", last.unwrap())
}

/// What gets embedded: names and labels only, never contents.
pub fn doc_text(r: &Record) -> String {
    let src = r.sources.first().or(r.was.first()).map(|s| s.as_str()).unwrap_or("");
    // names are other people's text: cleaned and capped before they reach any model (ordinary names are unchanged)
    format!("search_document: {} | {} | {}", crate::bundle::clean(src, 300), crate::bundle::clean(&r.kind, 60), crate::bundle::clean(&r.tags.join(" "), 200))
}

fn text_hash(t: &str) -> String {
    format!("{:x}", Sha256::digest(t.as_bytes()))[..16].to_string()
}

pub fn embed(recs: &[Record], dir: &Path, host: &str, model: &str, all: bool) -> Result<()> {
    ensure_local(host)?;
    let existing: HashMap<String, Row> = load(dir, model)?.into_iter().map(|r| (r.id.clone(), r)).collect();
    let eligible: Vec<&Record> = recs
        .iter()
        .filter(|r| r.status != "gone" && (all || !r.tags.iter().any(|t| t == "derived")))
        .collect();
    let mut keep: Vec<Row> = Vec::new();
    let mut todo: Vec<(String, String, String)> = Vec::new(); // id, text, hash
    let mut reused = 0;
    let mut existing = existing;
    for r in &eligible {
        let t = doc_text(r);
        let h = text_hash(&t);
        match existing.remove(&r.id) {
            Some(row) if row.text_hash == h => {
                keep.push(row);
                reused += 1;
            }
            _ => todo.push((r.id.clone(), t, h)),
        }
    }
    eprintln!("{} eligible records: {} already embedded, {} to embed", eligible.len(), reused, todo.len());
    let started = std::time::Instant::now();
    for (i, chunk) in todo.chunks(BATCH).enumerate() {
        let texts: Vec<String> = chunk.iter().map(|c| c.1.clone()).collect();
        let vecs = embed_batch(host, model, &texts)?;
        for (c, v) in chunk.iter().zip(vecs) {
            keep.push(Row { id: c.0.clone(), text_hash: c.2.clone(), vec: v });
        }
        if (i + 1) % 10 == 0 {
            eprintln!("  {}/{} ({:.0}s)", (i + 1) * BATCH.min(todo.len()), todo.len(), started.elapsed().as_secs_f32());
        }
    }
    save(dir, model, &keep)?;
    println!("embedded {} new, kept {}, total {} vectors in {:.1}s -> {}", todo.len(), reused, keep.len(), started.elapsed().as_secs_f32(), dir.display());
    Ok(())
}

/// Ranked (score, record id) pairs for a free-text question.
pub fn search(recs: &[Record], dir: &Path, host: &str, model: &str, query: &str) -> Result<Vec<(f32, String)>> {
    ensure_local(host)?;
    let rows = load(dir, model)?;
    if rows.is_empty() {
        bail!("no embeddings yet: run `lifestream embed` first");
    }
    let q = embed_batch(host, model, &[format!("search_query: {query}")])?.remove(0);
    let live: std::collections::HashSet<&str> = recs.iter().filter(|r| r.status != "gone").map(|r| r.id.as_str()).collect();
    let mut scored: Vec<(f32, String)> = rows
        .iter()
        .filter(|row| live.contains(row.id.as_str()))
        .map(|row| (row.vec.iter().zip(&q).map(|(a, b)| a * b).sum::<f32>(), row.id.clone()))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored)
}

pub fn ask(recs: &[Record], dir: &Path, host: &str, model: &str, query: &str, limit: usize) -> Result<()> {
    let by_id: HashMap<&str, &Record> = recs.iter().map(|r| (r.id.as_str(), r)).collect();
    for (s, id) in search(recs, dir, host, model, query)?.iter().take(limit) {
        let Some(r) = by_id.get(id.as_str()) else { continue };
        let src = r.sources.first().map(|s| s.as_str()).unwrap_or("");
        let extra = if r.sources.len() > 1 { format!(" (+{})", r.sources.len() - 1) } else { String::new() };
        println!("{s:.3} {} {:>9} {:20} {}{}", &r.mtime[..10], human(r.size), r.kind, src, extra);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_hosts_allowed() {
        assert!(ensure_local("http://127.0.0.1:11434").is_ok());
        assert!(ensure_local("http://localhost:11434").is_ok());
        assert!(ensure_local("http://[::1]:11434").is_ok());
        assert!(ensure_local("http://192.168.1.5:11434").is_err());
        assert!(ensure_local("https://api.example.com").is_err());
        assert!(ensure_local("http://127.0.0.1.evil.com").is_err());
    }

    #[test]
    fn disguised_hosts_are_refused() {
        for h in ["http://127.0.0.1:80@evil.example", "http://127.0.0.1@evil.example", "http://localhost@evil.example/", "http://[fd00::5]:11434",
                  "http://[::1]x@evil.example", "http://localhost.evil.example", "http://0x7f.1@evil.example", "http://127.0.0.1:abc", "http://",
                  "ftp://127.0.0.1", "127.0.0.1:11434", "http://127.0.0.1\\@evil.example", "http://[::ffff:8.8.8.8]:11434", "http://10.0.0.1", "http://localhost%2f@evil.example"] {
            assert!(ensure_local(h).is_err(), "should refuse {h}");
        }
        for h in ["http://127.0.0.1", "http://127.0.0.1:11434/", "http://LOCALHOST:11434", "http://127.1.2.3:80", "http://[::1]", "https://localhost/api"] {
            assert!(ensure_local(h).is_ok(), "should allow {h}");
        }
    }

    #[test]
    fn store_round_trip() {
        let d = std::env::temp_dir().join(format!("ls-emb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let rows = vec![
            Row { id: "sha256:a".into(), text_hash: "h1".into(), vec: vec![0.5, -0.5, 0.25] },
            Row { id: "repo:b".into(), text_hash: "h2".into(), vec: vec![1.0, 0.0, -1.0] },
        ];
        save(&d, "nomic-embed-text:latest", &rows).unwrap();
        let back = load(&d, "nomic-embed-text:latest").unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].id, "repo:b");
        assert_eq!(back[0].vec, vec![0.5, -0.5, 0.25]);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn normalize_gives_unit_length() {
        let v = normalize(vec![3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }
}
