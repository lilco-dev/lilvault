use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: Server,
    pub vaults: Vec<Vault>,
    pub agents: Vec<Agent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default = "default_hosts")]
    pub mcp_allowed_hosts: Vec<String>,
}

fn default_hosts() -> Vec<String> {
    vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
}

fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8737))
}
impl Default for Server {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            mcp_allowed_hosts: default_hosts(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vault {
    pub id: String,
    pub path: PathBuf,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub token_env: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub &'static str);

impl Config {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        // TOML diagnostics can echo lines containing accidentally inlined secrets.
        toml::from_str(text).map_err(|_| ConfigError("invalid TOML or unsupported configuration fields; secrets must be environment variables"))
    }
}
