//! GPU canvas renderer for desigma documents.
//!
//! See `README.md` for the backend decision. The CPU renderer in
//! `desigma-render` stays the deterministic reference; this crate is held to it
//! by the fidelity tests (`tests/fidelity.rs`).
//!
//! Usage: a [`SceneCompiler`] turns the visible part of a page into a
//! [`Frame`] (CPU, needs the document); a [`Renderer`] executes frames on the
//! GPU. The editor compiles while painting its UI and executes inside egui's
//! paint callback; [`HeadlessGpu`] does both for tests and screenshots.

mod canvas;
mod compile;
mod context;
mod exec;
mod frame;
mod geom;
mod headless;
mod mesh;
mod paintconv;
mod pipelines;
mod present;
mod resources;

pub use canvas::Canvas;
pub use compile::{CompileOptions, SceneCompiler};
pub use context::{GpuContext, GpuError};
pub use exec::{ExecStats, Output, Renderer};
pub use frame::{Frame, FrameStats};
pub use headless::HeadlessGpu;
pub use present::{PresentView, Presenter};
pub use wgpu;
