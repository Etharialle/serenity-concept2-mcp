use clap::{Parser, ValueEnum};

use crate::api::{ApiClient, ApiEnvironment, ApiError};

/// A local, read-only MCP connection to the Concept2 Logbook.
#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    /// Select the Concept2 API environment. Tokens are environment-specific.
    #[arg(long, value_enum, default_value_t = Environment::Production)]
    pub environment: Environment,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Environment {
    Production,
    Development,
}

impl Cli {
    pub fn api_client(&self) -> Result<ApiClient, ApiError> {
        let token = std::env::var("CONCEPT2_ACCESS_TOKEN").map_err(|_| ApiError::Configuration)?;
        let environment = match self.environment {
            Environment::Production => ApiEnvironment::Production,
            Environment::Development => ApiEnvironment::Development,
        };
        ApiClient::new(token, environment)
    }
}
