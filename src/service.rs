use crate::{
    config::{Config, ConfigError},
    error::{self, Error},
    vault::{Condition, Entry, Match, Note, Vault},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::SocketAddr,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
};
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Read,
    ReadWrite,
}

#[derive(Clone)]
pub struct Agent {
    name: String,
    permissions: HashMap<String, Access>,
    auth_manager: bool,
}
impl Agent {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn auth_manager(&self) -> bool {
        self.auth_manager
    }
}
struct Credential {
    digest: [u8; 32],
    agent: Agent,
}

#[derive(Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub permissions: Vec<String>,
}

#[derive(Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub access: Access,
}

#[derive(Serialize)]
pub struct VaultStatus {
    pub vault_id: String,
    pub access: Access,
    pub local_available: bool,
    pub sync: String,
}

pub struct Service {
    pub listen: SocketAddr,
    pub(crate) mcp_allowed_hosts: Vec<String>,
    vaults: RwLock<BTreeMap<String, Arc<Vault>>>,
    registry: Option<Registry>,
    credentials: RwLock<Vec<Credential>>,
    keys_dir: Option<PathBuf>,
}

struct Registry {
    dir: PathBuf,
    root: PathBuf,
}

pub(crate) async fn work<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| Error::Unavailable)?
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn safe_regular_file(metadata: &std::fs::Metadata, max_bytes: u64) -> bool {
    metadata.is_file() && metadata.len() <= max_bytes && metadata.nlink() == 1
}

fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// Parses a permission list into (vault access, auth-manager flag).
/// `auth:manage` is the reserved pseudo-vault that grants the
/// /v1/auth/agents minting surface; everything else must be
/// `vault:read` or `vault:read-write` against a known vault.
fn parse_permissions(
    permissions: Vec<String>,
    vaults: &BTreeMap<String, Arc<Vault>>,
) -> Result<(HashMap<String, Access>, bool), ConfigError> {
    let mut access = HashMap::new();
    let mut auth_manager = false;
    for permission in permissions {
        if permission == "auth:manage" {
            if auth_manager {
                return Err(ConfigError("duplicate auth:manage permission for an agent"));
            }
            auth_manager = true;
            continue;
        }
        let (vault, level) = permission.split_once(':').ok_or(ConfigError(
            "permissions must be vault:read, vault:read-write or auth:manage",
        ))?;
        if !vaults.contains_key(vault) {
            return Err(ConfigError(
                "permissions refer to an unknown or disabled vault",
            ));
        }
        let level = match level {
            "read" => Access::Read,
            "read-write" => Access::ReadWrite,
            _ => {
                return Err(ConfigError(
                    "permissions must be vault:read, vault:read-write or auth:manage",
                ));
            }
        };
        if access.insert(vault.to_owned(), level).is_some() {
            return Err(ConfigError("duplicate vault permission for an agent"));
        }
    }
    if access.is_empty() && !auth_manager {
        return Err(ConfigError("each agent requires at least one permission"));
    }
    Ok((access, auth_manager))
}

fn permission_strings(permissions: &HashMap<String, Access>, auth_manager: bool) -> Vec<String> {
    let mut out: Vec<String> = permissions
        .iter()
        .map(|(vault, level)| {
            format!(
                "{vault}:{}",
                match level {
                    Access::Read => "read",
                    Access::ReadWrite => "read-write",
                }
            )
        })
        .collect();
    out.sort();
    if auth_manager {
        out.push("auth:manage".into());
    }
    out
}

// shortcut: /dev/urandom — fine for the Linux/mac deployment targets; swap
// for the getrandom crate if this ever targets a non-POSIX host.
fn random_token() -> Result<String, Error> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .map_err(error::io)?
        .read_exact(&mut bytes)
        .map_err(error::io)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Writes a key file 0600 and never through a symlink — same intent as
/// lilvault-add-agent's `umask 077`, but not TOCTOU-open to a planted link.
fn write_private(path: &Path, contents: &str) -> Result<(), Error> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(
        path,
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from(0o600u32),
    )
    .map_err(|_| Error::Unavailable)?;
    rustix::io::write(&fd, contents.as_bytes()).map_err(|_| Error::Unavailable)?;
    Ok(())
}

/// Loads one token plus an explicit vault permission sidecar per agent.
fn load_keys(dir: &Path) -> Result<Vec<(String, String, Vec<String>)>, ConfigError> {
    let metadata = std::fs::symlink_metadata(dir)
        .map_err(|_| ConfigError("agent key directory is inaccessible"))?;
    if !metadata.is_dir() {
        return Err(ConfigError("agent key directory must be a real directory"));
    }
    let mut entries = Vec::new();
    for entry in
        std::fs::read_dir(dir).map_err(|_| ConfigError("agent key directory is unreadable"))?
    {
        let entry = entry.map_err(|_| ConfigError("agent key directory is unreadable"))?;
        let file_name = entry
            .file_name()
            .into_string()
            .map_err(|_| ConfigError("agent key file names must be UTF-8"))?;
        if file_name.starts_with('.') || file_name.ends_with(".permissions") {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| ConfigError("agent key file is inaccessible"))?;
        if !safe_regular_file(&metadata, 128) {
            return Err(ConfigError("agent key entries must be small regular files"));
        }
        let stem = file_name.strip_suffix(".token").ok_or(ConfigError(
            "agent key files must be named <agent>.token or <agent>.ro.token",
        ))?;
        let (name, read_only) = match stem.strip_suffix(".ro") {
            Some(name) => (name, true),
            None => (stem, false),
        };
        if !identifier(name) {
            return Err(ConfigError(
                "agent key file names contain an invalid agent name",
            ));
        }
        let token = std::fs::read_to_string(&path)
            .map_err(|_| ConfigError("agent key file is unreadable"))?
            .trim()
            .to_owned();
        let permission_path = dir.join(format!("{name}.permissions"));
        let permission_meta = std::fs::symlink_metadata(&permission_path)
            .map_err(|_| ConfigError("each agent key requires a matching .permissions file"))?;
        if !safe_regular_file(&permission_meta, 4096) {
            return Err(ConfigError(
                "agent permissions must be a small regular file",
            ));
        }
        let permissions = std::fs::read_to_string(permission_path)
            .map_err(|_| ConfigError("agent permissions file is unreadable"))?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if read_only
            && permissions
                .iter()
                .any(|permission| permission.ends_with(":read-write"))
        {
            return Err(ConfigError(
                "a .ro.token key cannot have read-write permissions",
            ));
        }
        entries.push((name.to_owned(), token, permissions));
    }
    Ok(entries)
}

fn discover_registry(registry: &Registry) -> Result<BTreeMap<String, Arc<Vault>>, Error> {
    for directory in [&registry.dir, &registry.root] {
        let metadata = std::fs::symlink_metadata(directory).map_err(error::io)?;
        if !metadata.file_type().is_dir() {
            return Err(Error::BadPath);
        }
    }
    let mut vaults = BTreeMap::new();
    let mut identities = HashSet::new();
    let entries = std::fs::read_dir(&registry.dir).map_err(error::io)?;
    for (count, entry) in entries.enumerate() {
        if count >= 1024 {
            return Err(Error::TooLarge);
        }
        let entry = entry.map_err(error::io)?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::BadPath)?;
        if id.starts_with('.') {
            continue;
        }
        if !identifier(&id) {
            return Err(Error::BadPath);
        }
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(error::io)?;
        if !safe_regular_file(&metadata, 1024) {
            return Err(Error::BadPath);
        }
        let path = registry.root.join(&id);
        let vault = Vault::open(&path)?;
        if !identities.insert(vault.identity().map_err(error::io)?) {
            return Err(Error::BadPath);
        }
        vaults.insert(id, Arc::new(vault));
    }
    Ok(vaults)
}

impl Service {
    pub fn from_config(
        config: Config,
        secret: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        if config.server.mcp_allowed_hosts.is_empty()
            || config.server.mcp_allowed_hosts.iter().any(|host| {
                host.is_empty()
                    || host.contains(['*', '/', '@'])
                    || host.chars().any(char::is_whitespace)
                    || host.chars().any(char::is_control)
            })
        {
            return Err(ConfigError(
                "mcp_allowed_hosts must contain explicit hostnames/IP authorities, not wildcard hosts, URLs, or whitespace",
            ));
        }
        if config.agents.is_empty() && config.auth.keys_dir.is_none() {
            return Err(ConfigError("at least one agent key source is required"));
        }
        let registry = match (config.vault_registry.dir, config.vault_registry.root) {
            (None, None) => None,
            (Some(dir), Some(root)) => {
                if !config.vaults.is_empty()
                    || !dir.is_absolute()
                    || !root.is_absolute()
                    || dir
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                    || root
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                {
                    return Err(ConfigError(
                        "vault registry requires absolute dir/root paths and cannot be combined with explicit vaults",
                    ));
                }
                let registry = Registry { dir, root };
                discover_registry(&registry).map_err(|_| {
                    ConfigError("vault registry is inaccessible or contains unsafe entries")
                })?;
                Some(registry)
            }
            _ => {
                return Err(ConfigError(
                    "vault registry requires both dir and root, or neither",
                ));
            }
        };
        let vaults = if let Some(registry) = &registry {
            discover_registry(registry).map_err(|_| {
                ConfigError("vault registry is inaccessible or contains unsafe entries")
            })?
        } else {
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
                    canonical.starts_with(other) || Path::new(other).starts_with(&canonical)
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
            vaults
        };
        let mut names = HashSet::new();
        let mut digests = HashSet::new();
        let mut credentials = Vec::new();
        let keys_dir = config.auth.keys_dir;
        let entries = if let Some(dir) = &keys_dir {
            if !config.agents.is_empty() {
                return Err(ConfigError(
                    "an agent key directory cannot be combined with explicit agent entries",
                ));
            }
            load_keys(dir)?
        } else {
            config
                .agents
                .into_iter()
                .map(|agent| {
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
                    Ok((agent.name, token, agent.permissions))
                })
                .collect::<Result<Vec<_>, ConfigError>>()?
        };
        if entries.is_empty() {
            tracing::warn!("no agent keys loaded; every request is denied until a key is added");
        }
        for (name, token, permissions) in entries {
            if !identifier(&name) || !names.insert(name.clone()) {
                return Err(ConfigError("agent names must be valid unique identifiers"));
            }
            if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ConfigError(
                    "each bearer token must be 32 random bytes encoded as 64 hex characters; generate with openssl rand -hex 32",
                ));
            }
            let digest = digest(&token);
            if !digests.insert(digest) {
                return Err(ConfigError("agent bearer tokens must be distinct"));
            }
            let (access, auth_manager) = parse_permissions(permissions, &vaults)?;
            credentials.push(Credential {
                digest,
                agent: Agent {
                    name,
                    permissions: access,
                    auth_manager,
                },
            });
        }
        Ok(Self {
            listen: config.server.listen,
            mcp_allowed_hosts: config.server.mcp_allowed_hosts,
            vaults: RwLock::new(vaults),
            registry,
            credentials: RwLock::new(credentials),
            keys_dir,
        })
    }

    pub fn authenticate(&self, token: &str) -> Option<Agent> {
        let candidate = digest(token);
        let credentials = self.credentials.read().ok()?;
        let mut result = None;
        for credential in credentials.iter() {
            if bool::from(credential.digest.ct_eq(&candidate)) {
                result = Some(credential.agent.clone());
            }
        }
        result
    }

    pub fn list_agents(&self) -> Result<Vec<AgentInfo>, Error> {
        let credentials = self.credentials.read().map_err(|_| Error::Unavailable)?;
        let mut agents: Vec<AgentInfo> = credentials
            .iter()
            .map(|credential| AgentInfo {
                name: credential.agent.name.clone(),
                permissions: permission_strings(
                    &credential.agent.permissions,
                    credential.agent.auth_manager,
                ),
            })
            .collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(agents)
    }

    /// Mints (or rotates) an agent token: the 64-hex token is returned once
    /// and only its digest is kept in memory. Persistence is the
    /// `auth.keys_dir` pair `<name>.token` + `<name>.permissions`; without a
    /// key directory there is nowhere durable to put a minted token, so the
    /// call is refused rather than minting something a restart would drop.
    pub fn mint_agent(&self, name: &str, permissions: Vec<String>) -> Result<String, Error> {
        let dir = self.keys_dir.as_ref().ok_or(Error::KeysDirRequired)?;
        if !identifier(name) {
            return Err(Error::BadRequest);
        }
        let vaults = self.current_vaults()?;
        let (access, auth_manager) =
            parse_permissions(permissions, &vaults).map_err(|_| Error::BadRequest)?;
        let token = random_token()?;
        // Sidecar first: if the token write then fails, the previous token and
        // its old permissions remain a valid pair and the mint reports an error.
        let sidecar = permission_strings(&access, auth_manager).join("\n") + "\n";
        write_private(&dir.join(format!("{name}.permissions")), &sidecar)?;
        write_private(&dir.join(format!("{name}.token")), &token)?;
        let mut credentials = self.credentials.write().map_err(|_| Error::Unavailable)?;
        credentials.retain(|credential| credential.agent.name != name);
        credentials.push(Credential {
            digest: digest(&token),
            agent: Agent {
                name: name.to_owned(),
                permissions: access,
                auth_manager,
            },
        });
        Ok(token)
    }

    pub fn revoke_agent(&self, name: &str) -> Result<(), Error> {
        let dir = self.keys_dir.as_ref().ok_or(Error::KeysDirRequired)?;
        let mut credentials = self.credentials.write().map_err(|_| Error::Unavailable)?;
        let before = credentials.len();
        credentials.retain(|credential| credential.agent.name != name);
        if credentials.len() == before {
            return Err(Error::NotFound);
        }
        for file in [format!("{name}.token"), format!("{name}.ro.token")] {
            let path = dir.join(file);
            if path.exists() {
                std::fs::remove_file(path).map_err(error::io)?;
            }
        }
        let _ = std::fs::remove_file(dir.join(format!("{name}.permissions")));
        Ok(())
    }

    fn current_vaults(&self) -> Result<BTreeMap<String, Arc<Vault>>, Error> {
        if let Some(registry) = &self.registry {
            let discovered = discover_registry(registry)?;
            // A failed refresh must leave the last complete snapshot intact.
            *self.vaults.write().map_err(|_| Error::Unavailable)? = discovered;
        }
        self.vaults
            .read()
            .map(|vaults| vaults.clone())
            .map_err(|_| Error::Unavailable)
    }

    pub fn ready(&self) -> bool {
        self.current_vaults()
            .map(|vaults| !vaults.is_empty() && vaults.values().all(|vault| vault.available()))
            .unwrap_or(false)
    }

    pub fn list_vaults(&self, agent: &Agent) -> Result<Vec<VaultInfo>, Error> {
        Ok(self
            .current_vaults()?
            .keys()
            .filter_map(|id| {
                agent.permissions.get(id).map(|&access| VaultInfo {
                    id: id.clone(),
                    access,
                })
            })
            .collect())
    }

    fn vault(&self, agent: &Agent, id: &str, write: bool) -> Result<Arc<Vault>, Error> {
        // Hide unauthorized vault existence; only authorized read-only callers receive 403.
        let access = agent.permissions.get(id).ok_or(Error::NotFound)?;
        if write && !matches!(access, Access::ReadWrite) {
            return Err(Error::Forbidden);
        }
        self.current_vaults()?
            .get(id)
            .cloned()
            .ok_or(Error::NotFound)
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
    pub fn delete(
        &self,
        agent: &Agent,
        id: &str,
        path: &str,
        condition: &Condition,
    ) -> Result<(), Error> {
        self.vault(agent, id, true)?.delete(path, condition)
    }
    pub fn search(
        &self,
        agent: &Agent,
        id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Match>, Error> {
        self.vault(agent, id, false)?.search(query, limit)
    }
    pub fn status(&self, agent: &Agent, id: &str) -> Result<VaultStatus, Error> {
        let access = agent.permissions.get(id).ok_or(Error::NotFound)?;
        let vault = self
            .current_vaults()?
            .get(id)
            .cloned()
            .ok_or(Error::NotFound)?;
        Ok(VaultStatus {
            vault_id: id.to_owned(),
            access: *access,
            local_available: vault.available(),
            // The sync process lives in a separate container; the gateway cannot
            // observe cloud state, so it stays honest rather than optimistic.
            sync: "unknown".into(),
        })
    }
}
