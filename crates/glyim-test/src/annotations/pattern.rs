use std::fmt;

#[derive(Clone, Debug)]
/// MatchPattern.
pub enum MatchPattern {
    /// Variant.
    Any,
    #[allow(missing_docs)]
    Substring(String),
    #[allow(missing_docs)]
    Regex(regex::Regex),
    #[allow(missing_docs)]
    Exact(String),
}

impl MatchPattern {
    /// substring.
    pub fn substring(s: &str) -> Self {
        Self::Substring(s.to_string())
    }
    /// exact.
    pub fn exact(s: &str) -> Self {
        Self::Exact(s.to_string())
    }
    /// regex.
    pub fn regex(pattern: &str) -> Result<Self, regex::Error> {
        Ok(Self::Regex(regex::Regex::new(pattern)?))
    }

    /// matches.
    pub fn matches(&self, message: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Substring(s) => message.contains(s.as_str()),
            Self::Regex(re) => re.is_match(message),
            Self::Exact(s) => message == s,
        }
    }

    /// description.
    pub fn description(&self) -> String {
        match self {
            Self::Any => "<any>".into(),
            Self::Substring(s) => format!("contains {:?}", s),
            Self::Regex(re) => format!("matches {:?}", re.as_str()),
            Self::Exact(s) => format!("== {:?}", s),
        }
    }
}

impl fmt::Display for MatchPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl PartialEq for MatchPattern {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Any, Self::Any) => true,
            (Self::Substring(a), Self::Substring(b)) => a == b,
            (Self::Exact(a), Self::Exact(b)) => a == b,
            (Self::Regex(a), Self::Regex(b)) => a.as_str() == b.as_str(),
            _ => false,
        }
    }
}
impl Eq for MatchPattern {}
