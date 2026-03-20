pub mod docker;
pub mod local;
pub mod ssh;

pub use docker::DockerTarget;
pub use local::LocalTarget;
pub use ssh::SshTarget;
