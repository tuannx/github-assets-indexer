pub mod adapters;
pub mod application;
pub mod cli;
pub mod domain;

pub use application::build_app;
pub use cli::run;
