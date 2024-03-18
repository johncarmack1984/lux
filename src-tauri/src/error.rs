use std::sync::PoisonError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Implement std::io::Error for our Error enum
    #[error(transparent)]
    Io(#[from] std::io::Error),
    // Add a PoisonError, but we implement it manually later
    #[error("the mutex was poisoned")]
    PoisonError(String),
}
// Implement Serialize for the error
impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
// Implement From<PoisonError> for Error to convert it to something we have set up serialization for
impl<T> From<PoisonError<T>> for Error {
    fn from(err: PoisonError<T>) -> Self {
        // We "just" convert the error to a string here
        Error::PoisonError(err.to_string())
    }
}
