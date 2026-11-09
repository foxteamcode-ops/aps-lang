#[path = "../compiler/mod.rs"]
pub mod compiler;

#[path = "../runtime/mod.rs"]
pub mod runtime;

#[path = "../tools/mod.rs"]
pub mod tools;

pub const APS_VERSION: &str = "0.1.0-alpha";
pub const APS_VERSION_DISPLAY: &str = "0.1.0-alpha";
pub const APS_RELEASE_STAGE: &str = "Alpha";
pub const APS_SNAPSHOT_ITERATION: usize = 1;
