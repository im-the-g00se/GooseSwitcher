//! Loading and in-memory indexing of Fedora's system Hunspell dictionaries.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use encoding_rs::KOI8_R;

use crate::recognition::{Dictionary as RecognitionDictionary, SystemDictionaries};

const FEDORA_HUNSPELL_DIRECTORY: &str = "/usr/share/hunspell";

/// Paths to the Russian and US English Hunspell file pairs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemDictionaryPaths {
    russian_aff: PathBuf,
    russian_dic: PathBuf,
    english_aff: PathBuf,
    english_dic: PathBuf,
}

impl SystemDictionaryPaths {
    /// Builds the standard `ru_RU` and `en_US` paths inside `directory`.
    pub fn from_directory(directory: impl AsRef<Path>) -> Self {
        let directory = directory.as_ref();
        Self {
            russian_aff: directory.join("ru_RU.aff"),
            russian_dic: directory.join("ru_RU.dic"),
            english_aff: directory.join("en_US.aff"),
            english_dic: directory.join("en_US.dic"),
        }
    }
}

impl Default for SystemDictionaryPaths {
    fn default() -> Self {
        Self::from_directory(FEDORA_HUNSPELL_DIRECTORY)
    }
}

enum LanguageDictionary {
    Empty,
    Loaded(Box<spellbook::Dictionary>),
}

impl RecognitionDictionary for LanguageDictionary {
    fn contains(&self, normalized_word: &str) -> bool {
        match self {
            Self::Empty => false,
            Self::Loaded(dictionary) => dictionary.check(normalized_word),
        }
    }
}

/// Russian and English Hunspell dictionaries parsed into memory.
pub struct SystemDictionaryIndex {
    russian: LanguageDictionary,
    english: LanguageDictionary,
}

impl fmt::Debug for SystemDictionaryIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SystemDictionaryIndex")
            .field(
                "russian_loaded",
                &matches!(self.russian, LanguageDictionary::Loaded(_)),
            )
            .field(
                "english_loaded",
                &matches!(self.english, LanguageDictionary::Loaded(_)),
            )
            .finish()
    }
}

impl SystemDictionaryIndex {
    /// Reads and parses both dictionaries once.
    pub fn load(paths: &SystemDictionaryPaths) -> Result<Self, DictionaryLoadError> {
        let russian = load_dictionary("ru_RU", &paths.russian_aff, &paths.russian_dic)?;
        let english = load_dictionary("en_US", &paths.english_aff, &paths.english_dic)?;
        Ok(Self { russian, english })
    }

    /// Borrows both indexes in the form consumed by the recognition engine.
    pub fn dictionaries(&self) -> SystemDictionaries<'_> {
        SystemDictionaries::new(&self.russian, &self.english)
    }

    /// Checks the Russian index.
    pub fn russian_contains(&self, normalized_word: &str) -> bool {
        self.russian.contains(normalized_word)
    }

    /// Checks the English index.
    pub fn english_contains(&self, normalized_word: &str) -> bool {
        self.english.contains(normalized_word)
    }
}

fn load_dictionary(
    locale: &'static str,
    aff_path: &Path,
    dic_path: &Path,
) -> Result<LanguageDictionary, DictionaryLoadError> {
    let aff_bytes = read(aff_path)?;
    let dic_bytes = read(dic_path)?;

    if dictionary_is_empty(&dic_bytes) {
        return Ok(LanguageDictionary::Empty);
    }

    let encoding = dictionary_encoding(&aff_bytes, aff_path)?;
    let aff = decode(&aff_bytes, aff_path, encoding)?;
    let dic = decode(&dic_bytes, dic_path, encoding)?;
    spellbook::Dictionary::new(&aff, &dic)
        .map(Box::new)
        .map(LanguageDictionary::Loaded)
        .map_err(|source| DictionaryLoadError::Parse { locale, source })
}

fn read(path: &Path) -> Result<Vec<u8>, DictionaryLoadError> {
    fs::read(path).map_err(|source| DictionaryLoadError::Read {
        path: path.to_owned(),
        source,
    })
}

#[derive(Clone, Copy)]
enum DictionaryEncoding {
    Utf8,
    Koi8R,
}

fn dictionary_encoding(
    aff_bytes: &[u8],
    path: &Path,
) -> Result<DictionaryEncoding, DictionaryLoadError> {
    let declared = aff_bytes
        .split(|byte| *byte == b'\n')
        .map(trim_ascii)
        .find_map(|line| line.strip_prefix(b"SET "))
        .map(trim_ascii);

    match declared {
        Some(b"UTF-8" | b"UTF8") => Ok(DictionaryEncoding::Utf8),
        Some(b"KOI8-R" | b"KOI8R") => Ok(DictionaryEncoding::Koi8R),
        Some(value) => Err(DictionaryLoadError::UnsupportedEncoding {
            path: path.to_owned(),
            encoding: String::from_utf8_lossy(value).into_owned(),
        }),
        None => Ok(DictionaryEncoding::Utf8),
    }
}

fn decode(
    bytes: &[u8],
    path: &Path,
    encoding: DictionaryEncoding,
) -> Result<String, DictionaryLoadError> {
    match encoding {
        DictionaryEncoding::Utf8 => {
            String::from_utf8(bytes.to_vec()).map_err(|_| DictionaryLoadError::InvalidEncoding {
                path: path.to_owned(),
                encoding: "UTF-8",
            })
        }
        DictionaryEncoding::Koi8R => KOI8_R
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|decoded| decoded.into_owned())
            .ok_or_else(|| DictionaryLoadError::InvalidEncoding {
                path: path.to_owned(),
                encoding: "KOI8-R",
            }),
    }
}

fn dictionary_is_empty(bytes: &[u8]) -> bool {
    let mut lines = bytes.split(|byte| *byte == b'\n').map(trim_ascii);
    match lines.next() {
        None | Some(b"") => lines.all(<[u8]>::is_empty),
        Some(b"0") => lines.all(<[u8]>::is_empty),
        Some(_) => false,
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// Failure while reading or parsing a system dictionary.
#[derive(Debug)]
pub enum DictionaryLoadError {
    /// A dictionary file could not be read.
    Read { path: PathBuf, source: io::Error },
    /// The dictionary declares an encoding unsupported by the loader.
    UnsupportedEncoding { path: PathBuf, encoding: String },
    /// Dictionary bytes are invalid in the declared encoding.
    InvalidEncoding {
        path: PathBuf,
        encoding: &'static str,
    },
    /// Hunspell data is malformed.
    Parse {
        locale: &'static str,
        source: spellbook::ParseDictionaryError,
    },
}

impl fmt::Display for DictionaryLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, .. } => write!(formatter, "failed to read {}", path.display()),
            Self::UnsupportedEncoding { path, encoding } => write!(
                formatter,
                "dictionary {} uses unsupported encoding {encoding:?}",
                path.display()
            ),
            Self::InvalidEncoding { path, encoding } => write!(
                formatter,
                "dictionary {} contains invalid {encoding}",
                path.display()
            ),
            Self::Parse { locale, source } => {
                write!(formatter, "failed to parse {locale} dictionary: {source}")
            }
        }
    }
}

impl Error for DictionaryLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::UnsupportedEncoding { .. }
            | Self::InvalidEncoding { .. }
            | Self::Parse { .. } => None,
        }
    }
}
