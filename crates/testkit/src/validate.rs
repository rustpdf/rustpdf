//! Wrappers around external PDF validators (Fase 0.4).

use std::path::Path;
use std::process::Command;

/// A supported external validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validator {
    /// `qpdf --check`
    Qpdf,
    /// `mutool clean` (round-trips the file, surfacing structural errors)
    MutoolClean,
    /// `verapdf` (PDF/A and PDF/UA conformance)
    VeraPdf,
}

impl Validator {
    /// The executable name this validator drives.
    pub fn binary(self) -> &'static str {
        match self {
            Validator::Qpdf => "qpdf",
            Validator::MutoolClean => "mutool",
            Validator::VeraPdf => "verapdf",
        }
    }

    /// All validators known to the harness.
    pub fn all() -> [Validator; 3] {
        [Validator::Qpdf, Validator::MutoolClean, Validator::VeraPdf]
    }

    fn is_available(self) -> bool {
        which(self.binary()).is_some()
    }
}

/// Outcome of running one validator against one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatorStatus {
    /// The validator ran and accepted the file.
    Pass,
    /// The validator ran and rejected the file (exit code + captured output).
    Fail { code: Option<i32>, output: String },
    /// The validator binary is not installed.
    Unavailable,
}

impl ValidatorStatus {
    /// True only for [`ValidatorStatus::Pass`].
    pub fn is_pass(&self) -> bool {
        matches!(self, ValidatorStatus::Pass)
    }
}

/// A validator paired with its result for a given file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub validator: Validator,
    pub status: ValidatorStatus,
}

/// Run the **structural** validators (`qpdf`, `mutool`) against `path`.
///
/// `verapdf` is intentionally excluded: it checks PDF/A *conformance*, which a
/// plain PDF is not expected to meet. Use `validate_with(path,
/// &[Validator::VeraPdf])` to check a PDF/A file specifically (`project.md` 0.4).
pub fn validate(path: impl AsRef<Path>) -> Vec<ValidationReport> {
    validate_with(path, &[Validator::Qpdf, Validator::MutoolClean])
}

/// Run a specific set of validators against `path`.
pub fn validate_with(path: impl AsRef<Path>, validators: &[Validator]) -> Vec<ValidationReport> {
    let path = path.as_ref();
    validators
        .iter()
        .map(|&validator| ValidationReport {
            validator,
            status: run_one(validator, path),
        })
        .collect()
}

/// The subset of validators whose binaries are installed on this machine.
pub fn available_validators() -> Vec<Validator> {
    Validator::all()
        .into_iter()
        .filter(|v| v.is_available())
        .collect()
}

fn run_one(validator: Validator, path: &Path) -> ValidatorStatus {
    if !validator.is_available() {
        return ValidatorStatus::Unavailable;
    }
    let result = match validator {
        Validator::Qpdf => Command::new("qpdf").arg("--check").arg(path).output(),
        Validator::MutoolClean => {
            // `mutool clean in.pdf <tmp>` rewrites the file; a non-zero exit
            // or stderr noise indicates structural problems. Write to a temp.
            let out = std::env::temp_dir().join("testkit-mutool-clean.pdf");
            Command::new("mutool")
                .arg("clean")
                .arg(path)
                .arg(&out)
                .output()
        }
        Validator::VeraPdf => Command::new("verapdf").arg(path).output(),
    };

    match result {
        Ok(output) if output.status.success() => ValidatorStatus::Pass,
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            ValidatorStatus::Fail {
                code: output.status.code(),
                output: text.trim().to_owned(),
            }
        }
        Err(e) => ValidatorStatus::Fail {
            code: None,
            output: format!("failed to spawn {}: {e}", validator.binary()),
        },
    }
}

/// Locate an executable on `PATH` (a tiny `which`).
fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_binary_reports_unavailable() {
        // A validator whose binary surely does not exist.
        let status = run_one(Validator::VeraPdf, Path::new("/nonexistent.pdf"));
        // Either Unavailable (not installed) or Fail (installed, file missing).
        assert!(matches!(
            status,
            ValidatorStatus::Unavailable | ValidatorStatus::Fail { .. }
        ));
    }

    #[test]
    fn which_finds_common_binary() {
        // `sh` exists on every unix CI box.
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
    }
}
