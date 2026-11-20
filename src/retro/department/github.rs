// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only GitHub HTTP boundary for resolved Cargo pins; injectable without credentials.
use super::{Evidence, Fact, Observation, PinFact, Scope};

/// Reads a repository lockfile and revision reachability through an injected GET boundary.
/// A missing dependency repository affects reachability alone, preserving revision counts.
pub fn collect_pins(
    scope: &Scope,
    observed_at: &str,
    mut get: impl FnMut(&str, bool) -> Result<String, String>,
) -> Evidence {
    let mut evidence = Evidence {
        source: "github".into(),
        observed_at: Some(observed_at.into()),
        ..Default::default()
    };
    for repository in &scope.repositories {
        let result = read_lock(scope, &repository.id, observed_at, &mut get);
        match result {
            Ok(rows) => evidence.observations.extend(rows),
            Err(reason) => {
                evidence
                    .reasons
                    .insert("pin".into(), format!("{}: {reason}", repository.id));
            }
        }
    }
    if !scope.repositories.is_empty() && !evidence.reasons.contains_key("pin") {
        evidence.complete.insert("pin".into());
    }
    evidence
}
fn read_lock(
    scope: &Scope,
    repository: &str,
    observed_at: &str,
    get: &mut impl FnMut(&str, bool) -> Result<String, String>,
) -> Result<Vec<Observation>, String> {
    if !valid_repo(repository) {
        return Err("invalid native repository selector".into());
    }
    let endpoint = format!("repos/{repository}/contents/Cargo.lock");
    let text = get(&endpoint, true)?;
    let lock: toml::Value = toml::from_str(&text).map_err(|e| format!("Cargo.lock: {e}"))?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("Cargo.lock.package missing")?;
    let mut rows = Vec::new();
    for package in packages {
        let Some(source) = package
            .get("source")
            .and_then(toml::Value::as_str)
            .filter(|s| s.starts_with("git+"))
        else {
            continue;
        };
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or("Cargo.lock.package.name missing")?;
        let (url, revision) = source
            .strip_prefix("git+")
            .and_then(|s| s.rsplit_once('#'))
            .ok_or("git lock source lacks resolved revision")?;
        if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("git lock source invalid resolved revision".into());
        }
        let base = url
            .split('?')
            .next()
            .ok_or("missing dependency repository")?;
        let dependency_repo = base
            .strip_prefix("https://github.com/")
            .map(|s| s.trim_end_matches(".git"));
        let on_main = dependency_repo.filter(|r| valid_repo(r)).and_then(|repo| {
            let body = get(&format!("repos/{repo}/compare/{revision}...HEAD"), false).ok()?;
            // GitHub compare: base revision is an ancestor for identical/ahead, not behind/diverged.
            let value: serde_json::Value = serde_json::from_str(&body).ok()?;
            match value.get("status").and_then(serde_json::Value::as_str)? {
                "identical" | "ahead" => Some(true),
                "behind" | "diverged" => Some(false),
                _ => None,
            }
        });
        let entity_id = format!("{repository}:{name}:{revision}");
        rows.push(Observation {
            event_id: format!("github:lock:{entity_id}"),
            entity_id,
            event_at: None,
            observed_at: Some(observed_at.into()),
            sequence: None,
            scope: scope.clone(),
            complete: true,
            source_ref: format!("github:{endpoint}#{name}"),
            attribution: None,
            fact: Fact::Pin(PinFact {
                repo: repository.into(),
                dependency: name.into(),
                revision: revision.into(),
                on_main,
            }),
        });
    }
    Ok(rows)
}
fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    matches!((parts.next(),parts.next(),parts.next()), (Some(a),Some(b),None) if !a.is_empty() && !b.is_empty() && a != "." && a != ".." && b != "." && b != ".." && repo.bytes().all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c)))
}
