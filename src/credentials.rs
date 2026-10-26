//! API keys, found without ever being printed.
//!
//! Each key is looked up in order: its environment variable, then
//! `credentials.toml` in the config directory. Kaggle additionally reads the
//! files its own CLI writes (`~/.kaggle/access_token`, then the legacy
//! `~/.kaggle/kaggle.json`), honoring `KAGGLE_CONFIG_DIR`. Keys are never
//! accepted as flags (docs/reference/contract.md), and nothing in this module
//! formats a secret into a message: callers learn only where a key came from.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    HuggingFace,
    Kaggle,
    DataGov,
    GitHub,
    Fred,
    Roboflow,
    DataCommons,
    Ncbi,
}

impl Key {
    pub const ALL: [Self; 8] = [
        Self::HuggingFace,
        Self::Kaggle,
        Self::DataGov,
        Self::GitHub,
        Self::Fred,
        Self::Roboflow,
        Self::DataCommons,
        Self::Ncbi,
    ];

    pub fn env_var(self) -> &'static str {
        match self {
            Self::HuggingFace => "HF_TOKEN",
            Self::Kaggle => "KAGGLE_API_TOKEN",
            Self::DataGov => "DATAGOV_API_KEY",
            Self::GitHub => "GITHUB_TOKEN",
            Self::Fred => "FRED_API_KEY",
            Self::Roboflow => "ROBOFLOW_API_KEY",
            Self::DataCommons => "DATACOMMONS_API_KEY",
            Self::Ncbi => "NCBI_API_KEY",
        }
    }

    /// The key's name inside `credentials.toml`.
    pub fn file_key(self) -> &'static str {
        match self {
            Self::HuggingFace => "huggingface",
            Self::Kaggle => "kaggle",
            Self::DataGov => "datagov",
            Self::GitHub => "github",
            Self::Fred => "fred",
            Self::Roboflow => "roboflow",
            Self::DataCommons => "datacommons",
            Self::Ncbi => "ncbi",
        }
    }

    /// Where a user gets one.
    pub fn signup(self) -> &'static str {
        match self {
            Self::HuggingFace => "https://huggingface.co/settings/tokens",
            Self::Kaggle => "https://www.kaggle.com/settings/api",
            Self::DataGov => "https://api.data.gov/signup/",
            Self::GitHub => "https://github.com/settings/tokens",
            Self::Fred => "https://fredaccount.stlouisfed.org/apikeys",
            Self::Roboflow => "https://app.roboflow.com/settings/api",
            Self::DataCommons => "https://apikeys.datacommons.org",
            Self::Ncbi => "https://account.ncbi.nlm.nih.gov/settings/",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Env(&'static str),
    File(PathBuf),
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env(var) => write!(f, "${var}"),
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// How a request proves who it is. Kaggle's legacy `kaggle.json` is the one
/// key that needs HTTP Basic instead of a bearer token.
#[derive(Clone)]
pub enum Secret {
    Token(String),
    Basic { user: String, key: String },
}

impl Secret {
    pub fn token(&self) -> &str {
        match self {
            Self::Token(t) => t,
            Self::Basic { key, .. } => key,
        }
    }

    /// The `Authorization` header value.
    pub fn authorization(&self) -> String {
        match self {
            Self::Token(t) => format!("Bearer {t}"),
            Self::Basic { user, key } => {
                format!("Basic {}", base64(format!("{user}:{key}").as_bytes()))
            }
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(redacted)")
    }
}

#[derive(Default)]
pub struct Credentials {
    found: HashMap<Key, (Secret, Origin)>,
    /// Key files that other users on the machine can read.
    pub loose_files: Vec<PathBuf>,
    /// A `credentials.toml` that exists but could not be read as a flat
    /// table, so it supplied no key.
    pub unread_file: Option<PathBuf>,
}

impl Credentials {
    pub fn load(config_dir: &Path) -> Self {
        let mut creds = Self::default();
        let file = config_dir.join("credentials.toml");
        let from_file = read_toml(&file).unwrap_or_else(|| {
            creds.unread_file = Some(file.clone());
            HashMap::new()
        });
        if is_loose(&file) {
            creds.loose_files.push(file.clone());
        }
        for key in Key::ALL {
            if let Some(value) = env(key.env_var()) {
                creds.found.insert(
                    key,
                    (Secret::Token(value), Origin::Env(key.env_var())),
                );
            } else if let Some(value) = from_file.get(key.file_key()) {
                creds.found.insert(
                    key,
                    (Secret::Token(value.clone()), Origin::File(file.clone())),
                );
            } else if key == Key::Kaggle
                && let Some((secret, path)) = kaggle_files()
            {
                if is_loose(&path) {
                    creds.loose_files.push(path.clone());
                }
                creds.found.insert(key, (secret, Origin::File(path)));
            }
        }
        creds
    }

    pub fn get(&self, key: Key) -> Option<&Secret> {
        self.found.get(&key).map(|(secret, _)| secret)
    }

    pub fn origin(&self, key: Key) -> Option<&Origin> {
        self.found.get(&key).map(|(_, origin)| origin)
    }
}

fn env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The table in `credentials.toml`, empty when there is no such file and
/// `None` when there is one that cannot be read as a table.
fn read_toml(path: &Path) -> Option<HashMap<String, String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => flat_table(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Some(HashMap::new())
        }
        Err(_) => None,
    }
}

/// `credentials.toml` as the flat table of `name = "value"` lines it is
/// documented to be, with comments, a leading byte order mark and either
/// kind of one-line string. Any other line, such as a `[table]` header, a
/// value that is not a string, a multi-line string or a name given twice,
/// makes the whole file unreadable, so a file is read as TOML reads it or
/// not at all.
fn flat_table(text: &str) -> Option<HashMap<String, String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut table = HashMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, rest) = if line.starts_with(['"', '\'']) {
            toml_string(line)?
        } else {
            let end = line.find(|c: char| {
                !(c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })?;
            let (name, rest) = line.split_at_checked(end)?;
            (!name.is_empty()).then(|| (name.to_owned(), rest))?
        };
        let rest = rest.trim_start().strip_prefix('=')?.trim_start();
        let (value, rest) = toml_string(rest)?;
        let rest = rest.trim_start();
        if !(rest.is_empty() || rest.starts_with('#'))
            || table.insert(name, value).is_some()
        {
            return None;
        }
    }
    Some(table)
}

/// A one-line TOML string at the start of `text`, `"basic"` with its escapes
/// or `'literal'` without, and the text after it.
fn toml_string(text: &str) -> Option<(String, &str)> {
    if text.starts_with("\"\"\"") || text.starts_with("'''") {
        return None;
    }
    if let Some(literal) = text.strip_prefix('\'') {
        let (value, rest) = literal.split_once('\'')?;
        let controls = value.chars().any(|c| c.is_control() && c != '\t');
        return (!controls).then(|| (value.to_owned(), rest));
    }
    let body = text.strip_prefix('"')?;
    let mut chars = body.char_indices();
    let mut value = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((value, body.get(i..)?.get(1..)?)),
            '\\' => value.push(match chars.next()?.1 {
                '"' => '"',
                '\\' => '\\',
                'b' => '\u{8}',
                't' => '\t',
                'n' => '\n',
                'f' => '\u{c}',
                'r' => '\r',
                'e' => '\u{1b}',
                'x' => unicode_escape(&mut chars, 2)?,
                'u' => unicode_escape(&mut chars, 4)?,
                'U' => unicode_escape(&mut chars, 8)?,
                _ => return None,
            }),
            c if c.is_control() && c != '\t' => return None,
            c => value.push(c),
        }
    }
    None
}

fn unicode_escape(
    chars: &mut impl Iterator<Item = (usize, char)>,
    digits: usize,
) -> Option<char> {
    let hex: String = chars.take(digits).map(|(_, c)| c).collect();
    let all_hex = hex.chars().all(|c| c.is_ascii_hexdigit());
    (hex.len() == digits && all_hex)
        .then(|| u32::from_str_radix(&hex, 16).ok())
        .flatten()
        .and_then(char::from_u32)
}

fn kaggle_dir() -> Option<PathBuf> {
    env("KAGGLE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| etcetera::home_dir().ok().map(|home| home.join(".kaggle")))
}

/// The token files the Kaggle CLI writes, newest scheme first.
fn kaggle_files() -> Option<(Secret, PathBuf)> {
    let dir = kaggle_dir()?;
    let token_path = dir.join("access_token");
    if let Ok(token) = std::fs::read_to_string(&token_path) {
        let token = token.trim();
        if !token.is_empty() {
            return Some((Secret::Token(token.to_owned()), token_path));
        }
    }
    let legacy_path = dir.join("kaggle.json");
    let legacy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&legacy_path).ok()?)
            .ok()?;
    let user = legacy.get("username")?.as_str()?.to_owned();
    let key = legacy.get("key")?.as_str()?.to_owned();
    Some((Secret::Basic { user, key }, legacy_path))
}

#[cfg(unix)]
fn is_loose(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.permissions().mode() & 0o077 != 0)
}

#[cfg(not(unix))]
fn is_loose(_path: &Path) -> bool {
    false
}

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard padded base64, for the one Basic header Kaggle's legacy key needs.
fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n =
            (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = [n >> 18, n >> 12, n >> 6, n];
        let emitted = chunk.len().saturating_add(1);
        for (i, sextet) in sextets.iter().enumerate() {
            if i < emitted {
                let index = usize::try_from(sextet & 63).unwrap_or(0);
                out.push(char::from(
                    ALPHABET.get(index).copied().unwrap_or(b'A'),
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_every_remainder_length() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"user:key"), "dXNlcjprZXk=");
    }

    #[test]
    fn secrets_never_debug_print_their_value() {
        let secret = Secret::Token("abc123".into());
        assert!(!format!("{secret:?}").contains("abc123"));
    }

    #[test]
    fn the_credentials_file_reads_as_toml_or_not_at_all() {
        let table = flat_table(
            "# keys\n\nfred = \"a\\\"b\\u00e9\" # comment\n  \
             'datagov' = 'C:\\raw'\nnc-bi_2=\"x\"\n",
        )
        .unwrap();
        assert_eq!(table.get("fred").map(String::as_str), Some("a\"bé"));
        assert_eq!(table.get("datagov").map(String::as_str), Some("C:\\raw"));
        assert_eq!(table.get("nc-bi_2").map(String::as_str), Some("x"));
        assert_eq!(flat_table("").unwrap().len(), 0);
        for refused in [
            "[keys]\nfred = \"a\"",
            "fred = 1",
            "fred = \"a\"\nfred = \"b\"",
            "fred = \"\"\"a\"\"\"",
            "fred = \"a\" b",
            "fred = \"unterminated",
            "fred = \"\\q\"",
            "fred.key = \"a\"",
            "= \"a\"",
            "fred",
        ] {
            assert_eq!(flat_table(refused), None, "{refused:?}");
        }
    }

    // The toml crate is the oracle: the file is TOML, read the way it was
    // before dataseek read it by hand.
    #[test]
    fn the_credentials_file_reads_as_the_toml_crate_reads_it() {
        for file in [
            "\u{feff}fred = \"a\"\n",
            "\u{feff}# keys\nfred = \"a\"",
            "fred = \"a\"\r\ndatagov = 'b'\r\n",
            "fred = \"a#b\" # \"quoted\" comment",
            "fred=\"a\"#tight",
            "fred = \"\\e[0m\\x41\\t\\u00e9\\U0001F600\"",
            "\"fred\" = \"a\"\nfred = \"b\"",
            "fred = \"\\ud800\"",
            "fred = \"\\x4\"",
            "fred = 'a'b'",
            "fred = 'a\u{0}b'",
            "fred = 'a\tb'",
            "[keys]",
            "fred = true",
        ] {
            let expected =
                toml::from_str::<HashMap<String, String>>(file).ok();
            assert_eq!(flat_table(file), expected, "{file:?}");
        }
    }

    #[test]
    fn an_unreadable_credentials_file_is_reported_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Credentials::load(dir.path()).unread_file, None);
        let file = dir.path().join("credentials.toml");
        std::fs::write(&file, "[keys]\nfred = \"a\"\n").unwrap();
        assert_eq!(Credentials::load(dir.path()).unread_file, Some(file));
    }

    #[test]
    fn the_credentials_file_supplies_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("credentials.toml"),
            "fred = \"from-file\"\n",
        )
        .unwrap();
        let creds = Credentials::load(dir.path());
        if std::env::var("FRED_API_KEY").is_err() {
            assert_eq!(creds.get(Key::Fred).unwrap().token(), "from-file");
            assert!(matches!(creds.origin(Key::Fred), Some(Origin::File(_))));
        }
    }
}
