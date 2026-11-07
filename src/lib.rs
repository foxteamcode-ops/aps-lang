#[path = "../compiler/mod.rs"]
pub mod compiler;

#[path = "../runtime/mod.rs"]
pub mod runtime;

#[path = "../tools/mod.rs"]
pub mod tools;

pub const APS_VERSION: &str = "1.0.0-snapshot.v2";
pub const APS_VERSION_DISPLAY: &str = "1.0.0 snapshot v2";
pub const APS_RELEASE_STAGE: &str = "Snapshot";
pub const APS_SNAPSHOT_ITERATION: usize = 2;
