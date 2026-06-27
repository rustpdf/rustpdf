//! Test infrastructure: external-validator harness (Fase 0.4) and perceptual
//! visual-regression helpers (Fase 0.5).
//!
//! Validators shell out to `qpdf`, `mutool` and `verapdf`. Any validator whose
//! binary is not installed is reported as [`ValidatorStatus::Unavailable`]
//! rather than failing the run, so the suite degrades gracefully on machines
//! that only have some tools.

mod validate;
mod visual;

pub use validate::{
    available_validators, validate, validate_with, ValidationReport, Validator, ValidatorStatus,
};
pub use visual::{render_to_png, visual_diff, VisualError};
