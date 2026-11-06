use lilvault::{
    config::{Agent, Config, Server, Vault},
    service::Service,
};
use std::{fs, os::unix::fs::symlink, path::Path};

fn config(path: &Path) -> Config {
    Config {
        server: Server::default(),
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
