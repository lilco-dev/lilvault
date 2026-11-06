use crate::{
    config::{Config, ConfigError},
    error::Error,
    vault::{Condition, Entry, Note, Vault},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::SocketAddr,
    path::Component,
    sync::Arc,
};
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Read,
    ReadWrite,
}

#[derive(Clone)]
pub struct Agent {
    permissions: HashMap<String, Access>,
}
struct Credential {
    digest: [u8; 32],
    agent: Agent,
}

#[derive(Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub access: Access,
}

pub struct Service {
    pub listen: SocketAddr,
    vaults: BTreeMap<String, Arc<Vault>>,
    credentials: Vec<Credential>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

impl Service {
    pub fn from_config(
        config: Config,
        secret: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        if config.vaults.is_empty() || config.agents.is_empty() {
            return Err(ConfigError(
                "at least one enabled vault and one agent are required",
            ));
        }
        let mut ids = HashSet::new();
        let mut identities = HashSet::new();
        let mut roots = Vec::new();
        let mut vaults = BTreeMap::new();
        for vault in config.vaults {
            if !identifier(&vault.id) || !ids.insert(vault.id.clone()) {
                return Err(ConfigError(
                    "vault identifiers must be unique, 1-64 ASCII letters/digits/underscore/hyphen",
                ));
            }
            if !vault.path.is_absolute()
                || vault
                    .path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(ConfigError(
                    "vault paths must be absolute and contain no parent traversal",
                ));
            }
            if !vault.enabled {
                continue;
            }
            let opened = Vault::open(&vault.path).map_err(|_| ConfigError("vault path is inaccessible or unsafe; Linux openat2 and a non-symlink directory are required"))?;
            if !identities.insert(
                opened
                    .identity()
                    .map_err(|_| ConfigError("vault root metadata is unavailable"))?,
            ) {
                return Err(ConfigError(
                    "enabled vault roots must not alias one another, including bind mounts",
                ));
            }
            let canonical = vault
                .path
                .canonicalize()
                .map_err(|_| ConfigError("vault path is inaccessible"))?;
            if roots.iter().any(|other| {
                canonical.starts_with(other) || std::path::Path::new(other).starts_with(&canonical)
            }) {
                return Err(ConfigError(
                    "enabled vault roots must not overlap or alias one another",
                ));
            }
            roots.push(canonical);
            vaults.insert(vault.id, Arc::new(opened));
        }
        if vaults.is_empty() {
            return Err(ConfigError("at least one enabled vault is required"));
        }
        let mut names = HashSet::new();
        let mut digests = HashSet::new();
        let mut credentials = Vec::new();
        for agent in config.agents {
            if !identifier(&agent.name) || !names.insert(agent.name) {
                return Err(ConfigError("agent names must be valid unique identifiers"));
            }
            if agent.token_env.is_empty()
                || agent.token_env.len() > 128
                || !agent
                    .token_env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(ConfigError(
                    "token_env must name an uppercase environment variable",
                ));
            }
            let token = secret(&agent.token_env).ok_or(ConfigError(
                "a configured agent bearer environment variable is missing",
            ))?;
            if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ConfigError(
                    "each bearer token must be 32 random bytes encoded as 64 hex characters; generate with openssl rand -hex 32",
                ));
            }
            let digest = digest(&token);
            if !digests.insert(digest) {
                return Err(ConfigError("agent bearer tokens must be distinct"));
            }
            let mut permissions = HashMap::new();
            if agent.permissions.is_empty() {
                return Err(ConfigError("each agent requires at least one permission"));
            }
            for permission in agent.permissions {
                let (vault, access) = permission.split_once(':').ok_or(ConfigError(
                    "permissions must be vault:read or vault:read-write",
                ))?;
                if !vaults.contains_key(vault) {
                    return Err(ConfigError(
                        "permissions refer to an unknown or disabled vault",
                    ));
                }
                let access = match access {
                    "read" => Access::Read,
                    "read-write" => Access::ReadWrite,
                    _ => {
                        return Err(ConfigError(
                            "permissions must be vault:read or vault:read-write",
                        ));
                    }
                };
                if permissions.insert(vault.to_owned(), access).is_some() {
                    return Err(ConfigError("duplicate vault permission for an agent"));
                }
            }
            credentials.push(Credential {
                digest,
                agent: Agent { permissions },
            });
        }
        Ok(Self {
            listen: config.server.listen,
            vaults,
            credentials,
        })
    }

    pub fn authenticate(&self, token: &str) -> Option<Agent> {
        let candidate = digest(token);
        let mut result = None;
        for credential in &self.credentials {
            if bool::from(credential.digest.ct_eq(&candidate)) {
                result = Some(credential.agent.clone());
            }
        }
        result
    }

    pub fn ready(&self) -> bool {
        self.vaults.values().all(|vault| vault.available())
    }

    pub fn list_vaults(&self, agent: &Agent) -> Vec<VaultInfo> {
        self.vaults
            .keys()
            .filter_map(|id| {
                agent.permissions.get(id).map(|&access| VaultInfo {
                    id: id.clone(),
                    access,
                })
            })
            .collect()
    }

    fn vault(&self, agent: &Agent, id: &str, write: bool) -> Result<&Vault, Error> {
        // Hide unauthorized vault existence; only authorized read-only callers receive 403.
        let access = agent.permissions.get(id).ok_or(Error::NotFound)?;
        if write && !matches!(access, Access::ReadWrite) {
            return Err(Error::Forbidden);
        }
        self.vaults.get(id).map(Arc::as_ref).ok_or(Error::NotFound)
    }

    pub fn read(&self, agent: &Agent, id: &str, path: &str) -> Result<Note, Error> {
        self.vault(agent, id, false)?.read(path)
    }
    pub fn list(&self, agent: &Agent, id: &str, path: &str) -> Result<Vec<Entry>, Error> {
        self.vault(agent, id, false)?.list(path)
    }
    pub fn write(
        &self,
        agent: &Agent,
        id: &str,
        path: &str,
        content: &str,
        condition: &Condition,
    ) -> Result<String, Error> {
        self.vault(agent, id, true)?.write(path, content, condition)
    }
}
