use lilvault::{
    config::{Agent, Auth, Config, Server, Vault, VaultRegistry},
    service::Service,
    vault::Condition,
};
use std::{fs, os::unix::fs::symlink, path::Path};

fn config(path: &Path) -> Config {
    Config {
        server: Server::default(),
        auth: Auth::default(),
        vault_registry: VaultRegistry::default(),
        vaults: vec![Vault {
            id: "a".into(),
            path: path.to_path_buf(),
            enabled: true,
        }],
        agents: vec![Agent {
            name: "agent".into(),
            token_env: "TOKEN".into(),
            permissions: vec!["a:read-write".into()],
        }],
    }
}
fn rejects(label: &str, mutate: impl FnOnce(&mut Config)) {
    let root = tempfile::tempdir().unwrap();
    let mut c = config(root.path());
    mutate(&mut c);
    assert!(
        Service::from_config(c, |_| Some("ab".repeat(32))).is_err(),
        "{label}"
    );
}

#[test]
fn configuration_fails_closed_on_malformed_registry_or_permissions() {
    rejects("empty MCP host allowlist", |c| {
        c.server.mcp_allowed_hosts.clear()
    });
    rejects("wildcard MCP host", |c| {
        c.server.mcp_allowed_hosts = vec!["*".into()]
    });
    rejects("URL instead of host", |c| {
        c.server.mcp_allowed_hosts = vec!["https://example.com".into()]
    });
    rejects("whitespace in MCP host", |c| {
        c.server.mcp_allowed_hosts = vec!["bad host".into()]
    });
    rejects("no vaults", |c| c.vaults.clear());
    rejects("no agents", |c| c.agents.clear());
    rejects("all disabled", |c| c.vaults[0].enabled = false);
    rejects("bad vault id", |c| c.vaults[0].id = "../escape".into());
    rejects("duplicate vault id", |c| {
        c.vaults.push(Vault {
            id: "a".into(),
            path: c.vaults[0].path.clone(),
            enabled: false,
        })
    });
    rejects("relative root", |c| c.vaults[0].path = "relative".into());
    rejects("parent traversal root", |c| {
        c.vaults[0].path = c.vaults[0].path.join("..")
    });
    rejects("inaccessible root", |c| {
        c.vaults[0].path = c.vaults[0].path.join("absent")
    });
    rejects("bad agent name", |c| c.agents[0].name = "bad name".into());
    rejects("bad env name", |c| {
        c.agents[0].token_env = "lowercase".into()
    });
    rejects("empty permissions", |c| c.agents[0].permissions.clear());
    rejects("unknown vault", |c| {
        c.agents[0].permissions = vec!["unknown:read".into()]
    });
    rejects("unknown access", |c| {
        c.agents[0].permissions = vec!["a:admin".into()]
    });
    rejects("malformed permission", |c| {
        c.agents[0].permissions = vec!["a".into()]
    });
    rejects("duplicate permission", |c| {
        c.agents[0].permissions.push("a:read".into())
    });
    rejects("disabled grant", |c| {
        c.vaults.push(Vault {
            id: "b".into(),
            path: c.vaults[0].path.join("unused"),
            enabled: false,
        });
        c.agents[0].permissions.push("b:read".into());
    });
    rejects("overlapping roots", |c| {
        let path = c.vaults[0].path.join("nested");
        fs::create_dir(&path).unwrap();
        c.vaults.push(Vault {
            id: "nested".into(),
            path,
            enabled: true,
        });
    });
    rejects("aliased roots", |c| {
        c.vaults.push(Vault {
            id: "alias".into(),
            path: c.vaults[0].path.clone(),
            enabled: true,
        })
    });
}

#[test]
fn missing_invalid_and_duplicate_tokens_are_rejected_without_echoing_values() {
    let root = tempfile::tempdir().unwrap();
    for value in [
        None,
        Some("short".to_owned()),
        Some("z".repeat(64)),
        Some(" ".repeat(64)),
    ] {
        let result = Service::from_config(config(root.path()), |_| value.clone());
        let error = result.err().expect("invalid token accepted").to_string();
        if let Some(value) = value {
            assert!(!error.contains(&value));
        }
    }
    let mut c = config(root.path());
    c.agents.push(Agent {
        name: "other".into(),
        token_env: "OTHER".into(),
        permissions: vec!["a:read".into()],
    });
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());
    let mut c = config(root.path());
    c.agents.push(Agent {
        name: "agent".into(),
        token_env: "OTHER".into(),
        permissions: vec!["a:read".into()],
    });
    assert!(
        Service::from_config(c, |key| Some(
            if key == "TOKEN" { "ab" } else { "cd" }.repeat(32)
        ))
        .is_err()
    );
}

#[test]
fn unknown_fields_and_inline_secrets_do_not_appear_in_errors() {
    let error = Config::parse("inline_secret = 'do-not-echo-this-fixture'")
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("do-not-echo-this-fixture"));
    let root = tempfile::tempdir().unwrap();
    let valid = format!(
        r#"
[[vaults]]
id="a"
path="{}"
[[agents]]
name="agent"
token_env="TOKEN"
permissions=["a:read"]
"#,
        root.path().display()
    );
    assert!(Config::parse(&valid).is_ok());
    assert!(Config::parse(&(valid + "unexpected='no'\n")).is_err());
}

#[test]
fn symlinked_roots_are_rejected_and_default_binding_is_loopback() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = root.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(Service::from_config(config(&link), |_| Some("ab".repeat(32))).is_err());
    let service = Service::from_config(config(&real), |_| Some("ab".repeat(32))).unwrap();
    assert!(service.listen.ip().is_loopback());
    assert!(service.authenticate(&"ab".repeat(32)).is_some());
    assert!(service.authenticate("bad").is_none());
}

fn keys_config(vault: &Path, keys: &Path) -> Config {
    let mut c = config(vault);
    c.agents.clear();
    c.auth.keys_dir = Some(keys.to_path_buf());
    c
}

#[test]
fn agent_keys_are_loaded_from_file_names_with_read_only_suffix() {
    use lilvault::service::Access;
    let vault = tempfile::tempdir().unwrap();
    let keys = tempfile::tempdir().unwrap();
    let service = Service::from_config(keys_config(vault.path(), keys.path()), |_| None).unwrap();
    assert!(service.authenticate(&"ab".repeat(32)).is_none());
    assert!(service.ready());

    fs::write(keys.path().join("writer.token"), "ab".repeat(32)).unwrap();
    fs::write(keys.path().join("writer.permissions"), "a:read-write\n").unwrap();
    fs::write(keys.path().join("reader.ro.token"), "cd".repeat(32)).unwrap();
    fs::write(keys.path().join("reader.permissions"), "a:read\n").unwrap();
    let service = Service::from_config(keys_config(vault.path(), keys.path()), |_| None).unwrap();
    let access = |token: &str| {
        let agent = service
            .authenticate(token)
            .expect("key should authenticate");
        service.list_vaults(&agent).unwrap()[0].access
    };
    assert_eq!(access(&"ab".repeat(32)), Access::ReadWrite);
    assert_eq!(access(&"cd".repeat(32)), Access::Read);
    assert!(service.authenticate("bad").is_none());
}

#[test]
fn agent_key_directory_rejects_unsafe_malformed_or_ambiguous_entries() {
    let vault = tempfile::tempdir().unwrap();
    let keys = tempfile::tempdir().unwrap();
    let entry = |name: &str, content: &str| {
        fs::write(keys.path().join(name), content).unwrap();
        let rejected =
            Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err();
        fs::remove_file(keys.path().join(name)).unwrap();
        assert!(rejected, "{name} was accepted");
    };
    entry("bad name!.token", &"ab".repeat(32));
    entry("notes.md", &"ab".repeat(32));
    entry("short.token", "abc");
    entry("nonalpha.token", &"zz".repeat(32));
    entry("oversized.token", &("ab".repeat(32) + &"cd".repeat(40)));

    fs::write(keys.path().join("dup.token"), "ab".repeat(32)).unwrap();
    fs::write(keys.path().join("dup.ro.token"), "cd".repeat(32)).unwrap();
    fs::write(keys.path().join("dup.permissions"), "a:read\n").unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
    fs::remove_file(keys.path().join("dup.ro.token")).unwrap();
    fs::remove_file(keys.path().join("dup.token")).unwrap();
    fs::remove_file(keys.path().join("dup.permissions")).unwrap();

    fs::create_dir(keys.path().join("sub.token")).unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
    fs::remove_dir(keys.path().join("sub.token")).unwrap();

    symlink(vault.path().join("note.md"), keys.path().join("link.token")).unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
    fs::remove_file(keys.path().join("link.token")).unwrap();

    fs::write(vault.path().join("shared-token"), "ab".repeat(32)).unwrap();
    fs::hard_link(
        vault.path().join("shared-token"),
        keys.path().join("hard.token"),
    )
    .unwrap();
    fs::write(keys.path().join("hard.permissions"), "a:read\n").unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
    fs::remove_file(keys.path().join("hard.token")).unwrap();
    fs::remove_file(keys.path().join("hard.permissions")).unwrap();

    fs::write(keys.path().join("hard.token"), "ab".repeat(32)).unwrap();
    fs::write(vault.path().join("shared-permissions"), "a:read\n").unwrap();
    fs::hard_link(
        vault.path().join("shared-permissions"),
        keys.path().join("hard.permissions"),
    )
    .unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
    fs::remove_file(keys.path().join("hard.token")).unwrap();
    fs::remove_file(keys.path().join("hard.permissions")).unwrap();

    fs::write(vault.path().join("keys.txt"), "ab".repeat(32)).unwrap();
    let file_keys = keys_config(vault.path(), &vault.path().join("keys.txt"));
    assert!(Service::from_config(file_keys, |_| None).is_err());
    let missing = keys_config(vault.path(), &vault.path().join("missing"));
    assert!(Service::from_config(missing, |_| None).is_err());
}

#[test]
fn agent_key_directory_cannot_be_combined_with_explicit_entries() {
    let vault = tempfile::tempdir().unwrap();
    let keys = tempfile::tempdir().unwrap();
    let mut c = keys_config(vault.path(), keys.path());
    c.agents.push(Agent {
        name: "agent".into(),
        token_env: "TOKEN".into(),
        permissions: vec!["a:read".into()],
    });
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());
}

#[test]
fn registry_vaults_are_hot_discovered_and_removed_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let registry = tempfile::tempdir().unwrap();
    for id in ["a", "b"] {
        fs::create_dir(root.path().join(id)).unwrap();
        fs::write(registry.path().join(id), format!("{id}\n")).unwrap();
    }
    let c = Config {
        server: Server::default(),
        auth: Auth::default(),
        vault_registry: VaultRegistry {
            dir: Some(registry.path().to_path_buf()),
            root: Some(root.path().to_path_buf()),
        },
        vaults: Vec::new(),
        agents: vec![Agent {
            name: "agent".into(),
            token_env: "TOKEN".into(),
            permissions: vec!["a:read".into(), "b:read-write".into()],
        }],
    };
    let service = Service::from_config(c, |_| Some("ab".repeat(32))).unwrap();
    let agent = service.authenticate(&"ab".repeat(32)).unwrap();
    assert_eq!(service.list_vaults(&agent).unwrap().len(), 2);
    fs::remove_file(registry.path().join("b")).unwrap();
    assert_eq!(service.list_vaults(&agent).unwrap().len(), 1);
    fs::write(registry.path().join("b"), "b\n").unwrap();
    assert_eq!(service.list_vaults(&agent).unwrap().len(), 2);
    symlink("a", registry.path().join("unsafe")).unwrap();
    assert!(service.list_vaults(&agent).is_err());
    fs::remove_file(registry.path().join("unsafe")).unwrap();
    assert_eq!(service.list_vaults(&agent).unwrap().len(), 2);
    fs::write(root.path().join("a").join("note.md"), "A only\n").unwrap();
    assert_eq!(
        service.read(&agent, "a", "note.md").unwrap().content,
        "A only\n"
    );
    assert!(
        service
            .write(&agent, "a", "no.md", "x", &Condition::Create)
            .is_err()
    );
    assert!(
        service
            .write(&agent, "b", "yes.md", "B only\n", &Condition::Create)
            .is_ok()
    );
    assert_eq!(
        service.read(&agent, "b", "yes.md").unwrap().content,
        "B only\n"
    );
}

#[test]
fn registry_configuration_and_entries_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let registry = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    fs::write(registry.path().join("a"), "a\n").unwrap();
    let mut c = config(root.path().join("a").as_path());
    c.vault_registry = VaultRegistry {
        dir: Some(registry.path().to_path_buf()),
        root: Some(root.path().to_path_buf()),
    };
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());

    symlink("a", registry.path().join("link")).unwrap();
    let c = Config {
        server: Server::default(),
        auth: Auth::default(),
        vault_registry: VaultRegistry {
            dir: Some(registry.path().to_path_buf()),
            root: Some(root.path().to_path_buf()),
        },
        vaults: Vec::new(),
        agents: vec![Agent {
            name: "agent".into(),
            token_env: "TOKEN".into(),
            permissions: vec!["a:read".into()],
        }],
    };
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());

    fs::remove_file(registry.path().join("link")).unwrap();
    fs::write(root.path().join("marker-source"), "a\n").unwrap();
    fs::hard_link(
        root.path().join("marker-source"),
        registry.path().join("hard"),
    )
    .unwrap();
    let c = Config {
        server: Server::default(),
        auth: Auth::default(),
        vault_registry: VaultRegistry {
            dir: Some(registry.path().to_path_buf()),
            root: Some(root.path().to_path_buf()),
        },
        vaults: Vec::new(),
        agents: vec![Agent {
            name: "agent".into(),
            token_env: "TOKEN".into(),
            permissions: vec!["a:read".into()],
        }],
    };
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());
    fs::remove_file(registry.path().join("hard")).unwrap();

    let holder = tempfile::tempdir().unwrap();
    symlink(registry.path(), holder.path().join("registry-link")).unwrap();
    let c = Config {
        server: Server::default(),
        auth: Auth::default(),
        vault_registry: VaultRegistry {
            dir: Some(holder.path().join("registry-link")),
            root: Some(root.path().to_path_buf()),
        },
        vaults: Vec::new(),
        agents: vec![Agent {
            name: "agent".into(),
            token_env: "TOKEN".into(),
            permissions: vec!["a:read".into()],
        }],
    };
    assert!(Service::from_config(c, |_| Some("ab".repeat(32))).is_err());
}

#[test]
fn key_permissions_are_explicit_and_vault_scoped() {
    let vault = tempfile::tempdir().unwrap();
    let keys = tempfile::tempdir().unwrap();
    fs::write(keys.path().join("agent.token"), "ab".repeat(32)).unwrap();
    fs::write(keys.path().join("agent.permissions"), "unknown:read\n").unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());

    fs::remove_file(keys.path().join("agent.token")).unwrap();
    fs::write(keys.path().join("agent.ro.token"), "ab".repeat(32)).unwrap();
    fs::write(keys.path().join("agent.permissions"), "a:read-write\n").unwrap();
    assert!(Service::from_config(keys_config(vault.path(), keys.path()), |_| None).is_err());
}
