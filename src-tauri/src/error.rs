use serde::Serialize;
// use serde_json::error;
// use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    // Tauri(#[from] tauri::Error),
    #[error(transparent)]
    Var(#[from] std::env::VarError),
    // Libsql(#[from] libsql::Error),
    // #[error("Other: `{0}`")]
    // Other(String),
}

// impl fmt::Display for Error {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match self {
//             Error::Io(err) => write!(f, "IO error: {}", err),
//             Error::Tauri(err) => write!(f, "Tauri error: {}", err),
//             Error::Var(err) => write!(f, "Environment variable error: {}", err),
//             Error::Libsql(err) => write!(f, "Libsql error: {}", err),
//         }
//     }
// }

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
