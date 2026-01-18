/// Error types for loading, saving, and simulation operations.

#[derive(Debug)]
pub enum SimulationError {
    Config(String),
    Conversion(String),
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimulationError::Config(msg) => write!(f, "Configuration error: {}", msg),
            SimulationError::Conversion(msg) => write!(f, "Conversion error: {}", msg),
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(msg) => write!(f, "IO error: {}", msg),
            LoadError::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

#[derive(Debug)]
pub enum SaveError {
    Io(String),
    Serialize(String),
    NoPath,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(msg) => write!(f, "IO error: {}", msg),
            SaveError::Serialize(msg) => write!(f, "Serialization error: {}", msg),
            SaveError::NoPath => write!(f, "No file path configured"),
        }
    }
}
