//! The secret-shaped path rule (ADR-0065 d4): one list that the anchor
//! scan of ADR-0057 d2, a moved check's path (d18) and `oml sync`'s refusal
//! to read a file (d6) all apply, so the three cannot drift.

/// File-name suffixes that name key material.
const SUFFIXES: &[&str] = &[
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".jks",
    ".keystore",
    ".ppk",
    ".kdbx",
];

/// Whole file names that hold credentials.
const NAMES: &[&str] = &[
    ".npmrc",
    ".pypirc",
    ".netrc",
    "_netrc",
    ".pgpass",
    ".htpasswd",
    ".git-credentials",
    ".dockercfg",
    "kubeconfig",
];

/// Whether any component of `path` names a secret-carrying file or
/// directory: `.env*`, key-material suffixes (`*.pem`, `*.key`, `*.p12`,
/// `*.pfx`, `*.jks`, `*.keystore`, `*.ppk`, `*.kdbx`), SSH identities
/// (`id_*`), `*credentials*`, `*secret*`, the credential dotfiles
/// (`.npmrc`, `.pypirc`, `.netrc`, `.pgpass`, `.htpasswd`,
/// `.git-credentials`, `.dockercfg`), a kubeconfig (`kubeconfig*`, or
/// `config` under `.kube/`) and a Docker client's `config.json` under
/// `.docker/`. Case-insensitive; a directory so named covers every file
/// under it.
#[must_use]
pub fn secret_path(path: &str) -> bool {
    let parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(str::to_lowercase)
        .collect();
    parts.iter().enumerate().any(|(i, name)| {
        let parent = i.checked_sub(1).map(|j| parts[j].as_str());
        name.starts_with(".env")
            || SUFFIXES.iter().any(|s| name.ends_with(s))
            || name.starts_with("id_")
            || name.contains("credentials")
            || name.contains("secret")
            || NAMES.contains(&name.as_str())
            || name.starts_with("kubeconfig")
            || (parent == Some(".kube") && name == "config")
            || (parent == Some(".docker") && name == "config.json")
    })
}
