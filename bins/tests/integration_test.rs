// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Integration test to exercise the programs in `bins`.

use pty::fork::*;
use serde_json::{json, Value};
use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime},
};

/// This test is inspired by https://github.com/alexjg/linkd-playground
#[test]
fn two_peers_and_a_seed() {
    let peer1_home = "/tmp/link-local-1";
    let peer2_home = "/tmp/link-local-2";
    let seed_home = "/tmp/seed-home";
    let passphrase = b"play\n";

    println!("\n== create lnk homes for two peers and one seed ==\n");
    let (is_parent, _) = run_lnk(LnkCmd::ProfileCreate, peer1_home, passphrase);
    if !is_parent {
        return;
    }
    let (is_parent, _) = run_lnk(LnkCmd::ProfileCreate, peer2_home, passphrase);
    if !is_parent {
        return;
    }
    let (is_parent, _) = run_lnk(LnkCmd::ProfileCreate, seed_home, passphrase);
    if !is_parent {
        return;
    }

    println!("\n== add ssh keys for each profile to the ssh-agent ==\n");
    let (is_parent, _) = run_lnk(LnkCmd::ProfileSshAdd, peer1_home, passphrase);
    if !is_parent {
        return;
    }
    let (is_parent, _) = run_lnk(LnkCmd::ProfileSshAdd, peer2_home, passphrase);
    if !is_parent {
        return;
    }
    let (is_parent, _) = run_lnk(LnkCmd::ProfileSshAdd, seed_home, passphrase);
    if !is_parent {
        return;
    }

    println!("\n== Creating local link 1 identity ==\n");
    let peer1_name = "sockpuppet1".to_string();
    let (is_parent, output) = run_lnk(LnkCmd::IdPersonCreate(peer1_name), peer1_home, passphrase);
    if !is_parent {
        return;
    }
    let v: Value = serde_json::from_str(&output).unwrap();
    let urn1 = v["urn"].as_str().unwrap().to_string();
    let (is_parent, _) = run_lnk(LnkCmd::IdLocalSet(urn1), peer1_home, passphrase);
    if !is_parent {
        return;
    }

    println!("\n== Creating local link 2 identity ==\n");
    let peer2_name = "sockpuppet2".to_string();
    let (is_parent, output) = run_lnk(LnkCmd::IdPersonCreate(peer2_name), peer2_home, passphrase);
    if !is_parent {
        return;
    }
    let v: Value = serde_json::from_str(&output).unwrap();
    let urn2 = v["urn"].as_str().unwrap().to_string();
    let (is_parent, _) = run_lnk(LnkCmd::IdLocalSet(urn2), peer2_home, passphrase);
    if !is_parent {
        return;
    }

    println!("\n== Create a local repository ==\n");
    let peer1_proj = format!("peer1_proj_{}", timestamp());
    let (is_parent, output) = run_lnk(
        LnkCmd::IdProjectCreate(peer1_proj.clone()),
        peer1_home,
        passphrase,
    );
    if !is_parent {
        return;
    }
    let v: Value = serde_json::from_str(&output).unwrap();
    let proj_urn = v["urn"].as_str().unwrap().to_string();
    println!("our project URN: {}", &proj_urn);

    println!("\n== Add the seed to the local peer seed configs ==\n");
    let (is_parent, seed_peer_id) = run_lnk(LnkCmd::ProfilePeer, seed_home, passphrase);
    if !is_parent {
        return;
    }
    let seed_endpoint = format!("{}@127.0.0.1:8799", &seed_peer_id);

    let (is_parent, peer1_profile) = run_lnk(LnkCmd::ProfileGet, peer1_home, passphrase);
    if !is_parent {
        return;
    }
    let peer1_seed = format!("{}/{}/seeds", peer1_home, peer1_profile);
    let mut peer1_f = File::create(peer1_seed).unwrap();
    peer1_f.write_all(seed_endpoint.as_bytes()).unwrap();

    let (is_parent, peer2_profile) = run_lnk(LnkCmd::ProfileGet, peer2_home, passphrase);
    if !is_parent {
        return;
    }
    let peer2_seed = format!("{}/{}/seeds", peer2_home, peer2_profile);
    let mut peer2_f = File::create(peer2_seed).unwrap();
    peer2_f.write_all(seed_endpoint.as_bytes()).unwrap();

    println!("\n== Start the seed linkd ==\n");
    let manifest_path = manifest_path();
    let mut linkd = spawn_linkd(seed_home, &manifest_path);

    // println!("\n== Start the peer 1 gitd ==\n");
    // let (is_parent, peer1_peer_id) = run_lnk(LnkCmd::ProfilePeer, peer1_home, passphrase);
    // if !is_parent {
    //     return;
    // }
    // let mut lnk_gitd = spawn_lnk_gitd(peer1_home, &manifest_path, &peer1_peer_id);

    println!("\n== Make some changes in the repo ==\n");
    env::set_current_dir(&peer1_proj).unwrap();
    let mut test_file = File::create("test").unwrap();
    test_file.write_all(b"test").unwrap();
    Command::new("git")
        .arg("add")
        .arg("test")
        .output()
        .expect("failed to do git add");
    let output = Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("test commit")
        .output()
        .expect("failed to do git commit");
    println!("git-commit: {:?}", &output);

    println!("\n== Add the linkd remote to the repo ==\n");
    let remote_url = format!("ssh://rad@127.0.0.1:9987/{}.git", &proj_urn);
    Command::new("git")
        .arg("remote")
        .arg("add")
        .arg("linkd")
        .arg(remote_url)
        .output()
        .expect("failed to do git remote add");

    clean_up_known_hosts();

    // _run_git_push();

    // linkd.kill().ok();
    // lnk_gitd.kill().ok();
}

enum LnkCmd {
    ProfileCreate,
    ProfileGet,
    ProfilePeer,
    ProfileSshAdd,
    IdPersonCreate(String),  // the associated string is "the person's name".
    IdLocalSet(String),      // the associated string is "urn".
    IdProjectCreate(String), // the associated string is "the project name".
}

/// Runs a `cmd` for `lnk_home`. Rebuilds `lnk` if necessary.
/// Return.0: true if this is the parent (i.e. test) process,
///           false if this is the child (i.e. lnk) process.
/// Return.1: an output that depends on the `cmd`.
fn run_lnk(cmd: LnkCmd, lnk_home: &str, passphrase: &[u8]) -> (bool, String) {
    let fork = Fork::from_ptmx().unwrap();
    if let Some(mut parent) = fork.is_parent().ok() {
        // Input the passphrase if necessary.
        match cmd {
            LnkCmd::ProfileCreate | LnkCmd::ProfileSshAdd => {
                parent.write_all(passphrase).unwrap();
                println!("{}: wrote passphase", lnk_home);
            },
            _ => {},
        }

        // Print the output and decode them if necessary.
        let buf_reader = BufReader::new(parent);
        let mut output = String::new();
        for line in buf_reader.lines() {
            let line = line.unwrap();
            println!("{}: {}", lnk_home, line);

            match cmd {
                LnkCmd::IdPersonCreate(ref _name) => {
                    if line.find("\"urn\":").is_some() {
                        output = line; // get the line with URN.
                    }
                },
                LnkCmd::IdProjectCreate(ref _name) => {
                    if line.find("\"urn\":").is_some() {
                        output = line; // get the line with URN.
                    }
                },
                LnkCmd::ProfileGet => {
                    output = line; // get the last line for profile id.
                },
                LnkCmd::ProfilePeer => {
                    output = line; // get the last line for peer id.
                },
                _ => {},
            }
        }

        (true, output)
    } else {
        // Child process is to run `lnk`.
        let manifest_path = manifest_path();

        // cargo run \
        // --manifest-path $LINK_CHECKOUT/bins/Cargo.toml \
        // -p lnk -- "$@"
        let mut lnk_cmd = Command::new("cargo");
        lnk_cmd
            .env("LNK_HOME", lnk_home)
            .arg("run")
            .arg("--manifest-path")
            .arg(manifest_path)
            .arg("-p")
            .arg("lnk")
            .arg("--");
        let full_cmd = match cmd {
            LnkCmd::ProfileCreate => lnk_cmd.arg("profile").arg("create"),
            LnkCmd::ProfileGet => lnk_cmd.arg("profile").arg("get"),
            LnkCmd::ProfilePeer => lnk_cmd.arg("profile").arg("peer"),
            LnkCmd::ProfileSshAdd => lnk_cmd.arg("profile").arg("ssh").arg("add"),
            LnkCmd::IdPersonCreate(name) => {
                let payload = json!({ "name": name });
                lnk_cmd
                    .arg("identities")
                    .arg("person")
                    .arg("create")
                    .arg("new")
                    .arg("--payload")
                    .arg(payload.to_string())
            },
            LnkCmd::IdLocalSet(urn) => lnk_cmd
                .arg("identities")
                .arg("local")
                .arg("set")
                .arg("--urn")
                .arg(urn),
            LnkCmd::IdProjectCreate(name) => {
                let payload = json!({"name": name, "default_branch": "master"});
                let project_path = format!("./{}", name);
                lnk_cmd
                    .arg("identities")
                    .arg("project")
                    .arg("create")
                    .arg("new")
                    .arg("--path")
                    .arg(project_path)
                    .arg("--payload")
                    .arg(payload.to_string())
            },
        };
        full_cmd.status().expect("lnk cmd failed:");

        (false, String::new())
    }
}

fn spawn_linkd(lnk_home: &str, manifest_path: &str) -> Child {
    let log_name = format!("linkd_{}.log", &timestamp());
    let log_file = File::create(&log_name).unwrap();

    Command::new("cargo")
    .arg("build")
    .arg("--target-dir")
    .arg("./target")
    .arg("--manifest-path")
    .arg(manifest_path)
    .arg("-p")
    .arg("linkd")
    .output()
    .expect("cargo build linkd failed");

    let child = Command::new("./target/debug/linkd")
        .env("RUST_BACKTRACE", "1")
        .arg("--lnk-home")
        .arg(lnk_home)
        .arg("--track")
        .arg("everything")
        .arg("--protocol-listen")
        .arg("127.0.0.1:8799")
        .stdout(Stdio::from(log_file))
        .spawn()
        .expect("linkd failed to start");
    println!("linkd stdout redirected to {}", &log_name);
    thread::sleep(Duration::from_secs(1));
    child
}

fn spawn_lnk_gitd(lnk_home: &str, manifest_path: &str, peer_id: &str) -> Child {
    let log_name = format!("lnk-gitd_{}.log", &timestamp());
    let log_file = File::create(&log_name).unwrap();
    let port = "9987";
    let xdg_runtime_dir = env!("XDG_RUNTIME_DIR");
    let rpc_socket = format!("{}/link-peer-{}-rpc.socket", xdg_runtime_dir, peer_id);

    Command::new("cargo")
        .arg("build")
        .arg("--target-dir")
        .arg("./target")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("-p")
        .arg("lnk-gitd")
        .output()
        .expect("cargo build lnk-gitd failed");

    let child = Command::new("systemd-socket-activate")
        .arg("-l")
        .arg(port)
        .arg("--fdname=ssh")
        .arg("-E")
        .arg("SSH_AUTH_SOCK")
        .arg("-E")
        .arg("RUST_BACKTRACE")
        .arg("./target/debug/lnk-gitd")
        .arg(lnk_home)
        .arg("--linkd-rpc-socket")
        .arg(rpc_socket)
        .arg("--push-seeds")
        .arg("--fetch-seeds")
        .arg("--linger-timeout")
        .arg("10000")
        .stdout(Stdio::from(log_file))
        .spawn()
        .expect("lnk-gitd failed to start");
    println!("lnk-gitd stdout redirected to {}", &log_name);
    thread::sleep(Duration::from_secs(1));

    child
}

/// Returns true if this is the parent process,
/// returns false if this is the child process.
fn _run_git_push() -> bool {
    let fork = Fork::from_ptmx().unwrap();
    if let Some(mut parent) = fork.is_parent().ok() {
        let yes = b"yes\n";
        parent.write_all(yes).unwrap();

        let buf_reader = BufReader::new(parent);
        for line in buf_reader.lines() {
            let line = line.unwrap();
            println!("git-push: {}", line);
        }

        true
    } else {
        Command::new("git")
            .arg("push")
            .arg("linkd")
            .status()
            .expect("failed to do git push");
        false
    }
}

/// Returns UNIX_TIME in millis.
fn timestamp() -> u128 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    now.as_millis()
}

/// Returns the full path of `bins` manifest file.
fn manifest_path() -> String {
    let package_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/Cargo.toml", package_dir.strip_suffix("/tests").unwrap())
}

fn clean_up_known_hosts() {
    // ssh-keygen -f "/home/pi/.ssh/known_hosts" -R "[127.0.0.1]:9987"
    let home_dir = env!("HOME");
    let known_hosts = format!("{}/.ssh/known_hosts", &home_dir);
    let output = Command::new("ssh-keygen")
        .arg("-f")
        .arg(known_hosts)
        .arg("-R")
        .arg("[127.0.0.1]:9987")
        .output()
        .expect("failed to do ssh-keygen");
    println!("ssh-keygen: {:?}", &output);
}
