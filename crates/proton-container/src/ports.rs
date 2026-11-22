//! Runtime port discovery (`/proc/net/tcp{,6}` + `docker port`) and attribute
//! merging against `portsAttributes` / `otherPortsAttributes`.

use std::collections::BTreeMap;

use crate::config::{PortAttributes, PortSpec};
use crate::manager::ForwardedPort;
use crate::runtime::{exec_args, ExecTarget};

const EPHEMERAL_MIN: u16 = 32768;

/// Parse Linux `/proc/net/tcp` or `/proc/net/tcp6` text for sockets in `0A`
/// (`LISTEN`) state, returning sorted unique non-ephemeral port numbers.
pub fn parse_proc_net_tcp(content: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("sl") {
            continue;
        }
        let mut cols = trimmed.split_whitespace();
        let _sl = cols.next();
        let Some(local_addr) = cols.next() else {
            continue;
        };
        let _rem_addr = cols.next();
        let Some(state) = cols.next() else {
            continue;
        };
        if !state.eq_ignore_ascii_case("0A") {
            continue;
        }
        let Some((local_ip, hex_port)) = local_addr.rsplit_once(':') else {
            continue;
        };
        // Skip Docker/Graviton's embedded DNS resolver at 127.0.0.11.
        if local_ip.eq_ignore_ascii_case("0B00007F") {
            continue;
        }
        let Ok(port) = u16::from_str_radix(hex_port, 16) else {
            continue;
        };
        if port > 0 && port < EPHEMERAL_MIN {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Probe currently listening TCP ports inside the container by reading
/// `/proc/net/tcp` and `/proc/net/tcp6`.
pub fn probe_listening_ports(target: &ExecTarget) -> Vec<u16> {
    let args = exec_args(
        target,
        false,
        "/bin/sh",
        &[
            "-c".to_string(),
            "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null || true".to_string(),
        ],
    );
    let Ok(out) = target.command().args(&args).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_proc_net_tcp(&String::from_utf8_lossy(&out.stdout))
}

/// Parse one line from `docker port <cid>` (e.g. `3000/tcp -> 0.0.0.0:3000`).
pub fn parse_docker_port_line(line: &str) -> Option<(u16, u16, String)> {
    let (left, right) = line.split_once("->")?;
    let left = left.trim();
    let (c_str, proto) = match left.split_once('/') {
        Some((p, pr)) => (p.trim(), pr.trim().to_string()),
        None => (left, "tcp".to_string()),
    };
    let container_port: u16 = c_str.parse().ok()?;
    let host_part = right.trim().lines().next()?.trim();
    let (_, h_str) = host_part.rsplit_once(':')?;
    let host_port: u16 = h_str.trim().parse().ok()?;
    Some((container_port, host_port, proto))
}

/// Merge configured `forwardPorts`, runtime `port` mappings, and dynamically
/// probed listening sockets, applying `portsAttributes` / `otherPortsAttributes`.
pub fn merge_ports(
    forward_ports: &[PortSpec],
    docker_port_output: &str,
    listening_ports: &[u16],
    ports_attributes: &BTreeMap<String, PortAttributes>,
    other_ports_attributes: Option<&PortAttributes>,
) -> Vec<ForwardedPort> {
    // Value: (host_port, protocol, auto_detected)
    let mut map: BTreeMap<u16, (u16, String, bool)> = BTreeMap::new();

    for p in forward_ports {
        map.insert(p.container_port, (p.host_port, "tcp".to_string(), false));
    }

    for line in docker_port_output.lines() {
        if let Some((c_port, h_port, proto)) = parse_docker_port_line(line) {
            map.entry(c_port)
                .and_modify(|entry| {
                    entry.0 = h_port;
                    entry.1 = proto.clone();
                })
                .or_insert((h_port, proto, true));
        }
    }

    for &c_port in listening_ports {
        map.entry(c_port)
            .or_insert_with(|| (c_port, "tcp".to_string(), true));
    }

    map.into_iter()
        .filter_map(|(container_port, (host_port, proto, auto_detected))| {
            let attr = ports_attributes
                .get(&container_port.to_string())
                .or(other_ports_attributes);
            if auto_detected
                && attr
                    .and_then(|a| a.on_auto_forward.as_deref())
                    .is_some_and(|v| v.eq_ignore_ascii_case("ignore"))
            {
                return None;
            }
            let label = attr.and_then(|a| a.label.clone());
            let protocol = attr.and_then(|a| a.protocol.clone()).unwrap_or(proto);
            Some(ForwardedPort {
                container_port,
                host_port,
                label,
                protocol,
                auto_detected,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_net_tcp_and_tcp6_listening_non_ephemeral_ports() {
        let sample = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0B00007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1000
   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1001
   2: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1002
   3: 00000000:9C40 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1003
   4: 00000000:1435 00000000:0000 01 00000000:00000000 00:00000000 00000000     0        0 1004
   sl  local_address                         remote_address                        st
   0: 00000000000000000000000000000000:46AB 00000000000000000000000000000000:0000 0A
   1: 00000000000000000000000000000000:0BB8 00000000000000000000000000000000:0000 0A
";
        // 0x1F90 = 8080, 0x0BB8 = 3000, 0x46AB = 18091; 127.0.0.11:53 skipped, 0x9C40 = 40000 (ephemeral filtered), 01 ignored
        assert_eq!(parse_proc_net_tcp(sample), vec![3000, 8080, 18091]);
    }

    #[test]
    fn merges_ports_with_other_attributes_and_ignore_filter() {
        let mut attrs = BTreeMap::new();
        attrs.insert(
            "3000".to_string(),
            PortAttributes {
                label: Some("Web".into()),
                protocol: Some("http".into()),
                on_auto_forward: None,
            },
        );
        attrs.insert(
            "9229".to_string(),
            PortAttributes {
                label: Some("Inspector".into()),
                protocol: None,
                on_auto_forward: Some("ignore".into()),
            },
        );
        let other = PortAttributes {
            label: Some("Auto Service".into()),
            protocol: Some("http".into()),
            on_auto_forward: None,
        };

        let merged = merge_ports(
            &[PortSpec {
                host_port: 3000,
                container_port: 3000,
            }],
            "3000/tcp -> 127.0.0.1:3000\n",
            &[3000, 5173, 9229],
            &attrs,
            Some(&other),
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].container_port, 3000);
        assert!(!merged[0].auto_detected);
        assert_eq!(merged[0].label.as_deref(), Some("Web"));
        assert_eq!(merged[1].container_port, 5173);
        assert!(merged[1].auto_detected);
        assert_eq!(merged[1].label.as_deref(), Some("Auto Service"));
    }
}
