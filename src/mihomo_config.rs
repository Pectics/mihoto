use std::{collections::BTreeMap, fs};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as YamlValue};

/// Mihomo top-level configuration keys whose contents are deliberately owned by
/// the subscription/profile rather than by Mihoto's local environment overlay.
///
/// These values are still preserved in the downloaded YAML.  They are rejected
/// only when someone tries to inject them through `mihomo_config.extra`.
pub const DYNAMIC_TOP_LEVEL_KEYS: &[&str] = &[
    "proxies",
    "proxy-groups",
    "rules",
    "proxy-providers",
    "rule-providers",
    "sub-rules",
    "tunnels",
];

pub type ValueMap = toml::map::Map<String, toml::Value>;

/// Sparse local overrides for Mihomo's non-dynamic configuration.
///
/// Every field is optional on purpose: an omitted TOML key does not produce a
/// YAML patch and therefore leaves the remote configuration untouched.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MihomoConfig {
    pub port: Option<u16>,
    pub socks_port: Option<u16>,
    pub mixed_port: Option<u16>,
    pub redir_port: Option<u16>,
    pub tproxy_port: Option<u16>,
    pub ss_config: Option<String>,
    pub vmess_config: Option<String>,
    pub inbound_tfo: Option<bool>,
    pub inbound_mptcp: Option<bool>,
    pub authentication: Option<Vec<String>>,
    pub skip_auth_prefixes: Option<Vec<String>>,
    pub lan_allowed_ips: Option<Vec<String>>,
    pub lan_disallowed_ips: Option<Vec<String>>,
    pub allow_lan: Option<bool>,
    pub bind_address: Option<String>,
    pub mode: Option<MihomoMode>,
    pub unified_delay: Option<bool>,
    pub log_level: Option<MihomoLogLevel>,
    pub ipv6: Option<bool>,
    pub external_controller: Option<String>,
    pub external_controller_routing_mark: Option<i32>,
    pub external_controller_pipe: Option<String>,
    pub external_controller_unix: Option<String>,
    pub external_controller_tls: Option<String>,
    pub external_controller_cors: Option<ExternalControllerCors>,
    pub external_ui: Option<String>,
    pub external_ui_url: Option<String>,
    pub external_ui_name: Option<String>,
    pub external_doh_server: Option<String>,
    pub secret: Option<String>,
    pub interface_name: Option<String>,
    pub routing_mark: Option<i32>,
    pub geo_auto_update: Option<bool>,
    pub geo_update_interval: Option<u16>,
    pub geodata_mode: Option<bool>,
    pub geodata_loader: Option<String>,
    pub geosite_matcher: Option<String>,
    pub global_client_fingerprint: Option<String>,
    pub global_ua: Option<String>,
    pub etag_support: Option<bool>,
    pub tcp_concurrent: Option<bool>,
    pub find_process_mode: Option<FindProcessMode>,
    pub keep_alive_idle: Option<u32>,
    pub keep_alive_interval: Option<u32>,
    pub disable_keep_alive: Option<bool>,
    pub geox_url: Option<GeoxUrl>,
    pub tls: Option<TlsConfig>,
    pub experimental: Option<ExperimentalConfig>,
    pub hosts: Option<ValueMap>,
    pub profile: Option<ProfileConfig>,
    pub tun: Option<TunConfig>,
    pub sniffer: Option<SnifferConfig>,
    pub dns: Option<DnsConfig>,
    pub ntp: Option<NtpConfig>,
    pub iptables: Option<IptablesConfig>,
    pub tuic_server: Option<TuicServerConfig>,
    pub clash_for_android: Option<ClashForAndroidConfig>,
    pub listeners: Option<Vec<ListenerConfig>>,
    #[serde(skip_serializing_if = "toml::map::Map::is_empty")]
    pub extra: ValueMap,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MihomoMode {
    #[serde(rename = "global", alias = "Global")]
    Global,
    #[serde(rename = "rule", alias = "Rule")]
    Rule,
    #[serde(rename = "direct", alias = "Direct")]
    Direct,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MihomoLogLevel {
    #[serde(rename = "silent", alias = "Silent")]
    Silent,
    #[serde(rename = "error", alias = "Error")]
    Error,
    #[serde(rename = "warning", alias = "Warning")]
    Warning,
    #[serde(rename = "info", alias = "Info")]
    Info,
    #[serde(rename = "debug", alias = "Debug")]
    Debug,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FindProcessMode {
    #[serde(rename = "always", alias = "Always")]
    Always,
    #[serde(rename = "strict", alias = "Strict")]
    Strict,
    #[serde(rename = "off", alias = "Off")]
    Off,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum DnsMode {
    #[serde(rename = "fake-ip", alias = "fake_ip")]
    FakeIp,
    #[serde(rename = "redir-host", alias = "redir_host")]
    RedirHost,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum DnsFilterMode {
    #[serde(rename = "blacklist", alias = "BlackList")]
    BlackList,
    #[serde(rename = "whitelist", alias = "WhiteList")]
    WhiteList,
    #[serde(rename = "rule", alias = "Rule")]
    Rule,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum TunStack {
    #[serde(rename = "system", alias = "System")]
    System,
    #[serde(rename = "gvisor", alias = "Gvisor")]
    Gvisor,
    #[serde(rename = "mixed", alias = "Mixed")]
    Mixed,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ExternalControllerCors {
    pub allow_origins: Option<Vec<String>>,
    pub allow_private_network: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct GeoxUrl {
    pub geoip: Option<String>,
    pub geosite: Option<String>,
    pub mmdb: Option<String>,
    pub asn: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    pub certificate: Option<String>,
    pub private_key: Option<String>,
    pub client_auth_type: Option<String>,
    pub client_auth_cert: Option<String>,
    pub ech_key: Option<String>,
    /// Mihomo currently spells this YAML key `custom-certifactes`.
    #[serde(rename = "custom_certifactes", alias = "custom_certificates")]
    pub custom_certifactes: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ExperimentalConfig {
    pub fingerprints: Option<Vec<String>>,
    pub quic_go_disable_gso: Option<bool>,
    pub quic_go_disable_ecn: Option<bool>,
    pub dialer_ip4p_convert: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub store_selected: Option<bool>,
    pub store_fake_ip: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TunConfig {
    pub enable: Option<bool>,
    pub device: Option<String>,
    pub stack: Option<TunStack>,
    pub dns_hijack: Option<Vec<String>>,
    pub auto_route: Option<bool>,
    pub auto_detect_interface: Option<bool>,
    pub mtu: Option<u32>,
    pub gso: Option<bool>,
    pub gso_max_size: Option<u32>,
    pub inet6_address: Option<Vec<String>>,
    pub iproute2_table_index: Option<i32>,
    pub iproute2_rule_index: Option<i32>,
    pub auto_redirect: Option<bool>,
    pub auto_redirect_input_mark: Option<u32>,
    pub auto_redirect_output_mark: Option<u32>,
    pub auto_redirect_iproute2_fallback_rule_index: Option<i32>,
    pub loopback_address: Option<Vec<String>>,
    pub strict_route: Option<bool>,
    pub route_address: Option<Vec<String>>,
    pub route_address_set: Option<Vec<String>>,
    pub route_exclude_address: Option<Vec<String>>,
    pub route_exclude_address_set: Option<Vec<String>>,
    pub include_interface: Option<Vec<String>>,
    pub exclude_interface: Option<Vec<String>>,
    pub include_uid: Option<Vec<u32>>,
    pub include_uid_range: Option<Vec<String>>,
    pub exclude_uid: Option<Vec<u32>>,
    pub exclude_uid_range: Option<Vec<String>>,
    pub exclude_src_port: Option<Vec<u16>>,
    pub exclude_src_port_range: Option<Vec<String>>,
    pub exclude_dst_port: Option<Vec<u16>>,
    pub exclude_dst_port_range: Option<Vec<String>>,
    pub include_android_user: Option<Vec<i32>>,
    pub include_package: Option<Vec<String>>,
    pub exclude_package: Option<Vec<String>>,
    pub include_mac_address: Option<Vec<String>>,
    pub exclude_mac_address: Option<Vec<String>>,
    pub endpoint_independent_nat: Option<bool>,
    pub udp_timeout: Option<i64>,
    pub icmp_timeout: Option<i64>,
    pub disable_icmp_forwarding: Option<bool>,
    pub file_descriptor: Option<i32>,
    pub inet4_route_address: Option<Vec<String>>,
    pub inet6_route_address: Option<Vec<String>>,
    pub inet4_route_exclude_address: Option<Vec<String>>,
    pub inet6_route_exclude_address: Option<Vec<String>>,
    pub recvmsgx: Option<bool>,
    pub sendmsgx: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FallbackFilter {
    pub geoip: Option<bool>,
    pub geoip_code: Option<String>,
    pub ipcidr: Option<Vec<String>>,
    pub domain: Option<Vec<String>>,
    pub geosite: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DnsConfig {
    pub enable: Option<bool>,
    pub prefer_h3: Option<bool>,
    pub ipv6: Option<bool>,
    pub ipv6_timeout: Option<u32>,
    pub use_hosts: Option<bool>,
    pub use_system_hosts: Option<bool>,
    pub respect_rules: Option<bool>,
    pub nameserver: Option<Vec<String>>,
    pub fallback: Option<Vec<String>>,
    pub fallback_filter: Option<FallbackFilter>,
    pub fallback_lazy_query: Option<bool>,
    pub listen: Option<String>,
    pub listen_routing_mark: Option<i32>,
    pub enhanced_mode: Option<DnsMode>,
    pub fake_ip_range: Option<String>,
    pub fake_ip_range6: Option<String>,
    pub fake_ip_filter: Option<Vec<String>>,
    pub fake_ip_filter_mode: Option<DnsFilterMode>,
    pub fake_ip_ttl: Option<u32>,
    pub default_nameserver: Option<Vec<String>>,
    pub cache_algorithm: Option<String>,
    pub cache_max_size: Option<u32>,
    pub nameserver_policy: Option<ValueMap>,
    pub proxy_server_nameserver: Option<Vec<String>>,
    pub proxy_server_nameserver_policy: Option<ValueMap>,
    pub direct_nameserver: Option<Vec<String>>,
    pub direct_nameserver_follow_policy: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SnifferConfig {
    pub enable: Option<bool>,
    pub override_destination: Option<bool>,
    pub sniffing: Option<Vec<String>>,
    pub force_domain: Option<Vec<String>>,
    pub skip_src_address: Option<Vec<String>>,
    pub skip_dst_address: Option<Vec<String>>,
    pub skip_domain: Option<Vec<String>>,
    pub port_whitelist: Option<Vec<String>>,
    pub force_dns_mapping: Option<bool>,
    pub parse_pure_ip: Option<bool>,
    pub sniff: Option<BTreeMap<String, SniffingConfig>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SniffingConfig {
    pub ports: Option<Vec<String>>,
    pub override_destination: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NtpConfig {
    pub enable: Option<bool>,
    pub server: Option<String>,
    pub port: Option<u16>,
    pub interval: Option<u32>,
    pub dialer_proxy: Option<String>,
    pub write_to_system: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct IptablesConfig {
    pub enable: Option<bool>,
    pub inbound_interface: Option<String>,
    pub bypass: Option<Vec<String>>,
    pub dns_redirect: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TuicServerConfig {
    pub enable: Option<bool>,
    pub listen: Option<String>,
    pub token: Option<Vec<String>>,
    pub users: Option<BTreeMap<String, String>>,
    pub certificate: Option<String>,
    pub private_key: Option<String>,
    pub congestion_controller: Option<String>,
    pub max_idle_time: Option<u32>,
    pub authentication_timeout: Option<u32>,
    pub alpn: Option<Vec<String>>,
    pub max_udp_relay_packet_size: Option<u32>,
    pub cwnd: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClashForAndroidConfig {
    pub append_system_dns: Option<bool>,
    pub ui_subtitle_pattern: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum ListenerPort {
    Number(u16),
    Range(String),
}

/// Common listener fields are typed.  Mihomo has listener-type-specific
/// fields, so the flattened map is a local, per-listener extension point; its
/// keys are converted from TOML snake_case to Mihomo kebab-case like the typed
/// fields.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct ListenerConfig {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub listener_type: Option<String>,
    pub port: Option<ListenerPort>,
    pub listen: Option<String>,
    pub routing_mark: Option<i32>,
    pub rule: Option<String>,
    pub proxy: Option<String>,
    pub udp: Option<bool>,
    #[serde(flatten)]
    pub extra: ValueMap,
}

impl MihomoConfig {
    /// Build a YAML mapping containing only explicitly configured local fields.
    pub fn to_yaml_patch(&self) -> Result<YamlValue> {
        let serialized = toml::to_string(self).context("serialize Mihomo TOML overrides")?;
        let value: toml::Value = toml::from_str(&serialized)
            .context("round-trip Mihomo TOML overrides before YAML conversion")?;
        let toml::Value::Table(mut table) = value else {
            bail!("Mihomo overrides must serialize as a TOML table");
        };

        let extra = table.remove("extra");
        let mut patch = match toml_to_yaml_value(&toml::Value::Table(table), &[]) {
            YamlValue::Mapping(mapping) => mapping,
            _ => unreachable!("a TOML table must convert to a YAML mapping"),
        };

        if let Some(toml::Value::Table(extra_table)) = extra {
            for (key, value) in extra_table {
                if DYNAMIC_TOP_LEVEL_KEYS.contains(&key.as_str()) {
                    bail!("mihomo_config.extra cannot override dynamic field `{key}`");
                }
                let yaml_key = YamlValue::String(key.clone());
                if patch.contains_key(&yaml_key) {
                    bail!("mihomo_config.extra duplicates typed field `{key}`");
                }
                patch.insert(
                    yaml_key,
                    toml_to_yaml_value(&value, &["extra".to_string(), key]),
                );
            }
        }

        Ok(YamlValue::Mapping(patch))
    }
}

pub fn apply_mihomo_override(path: &str, override_config: &MihomoConfig) -> Result<bool> {
    let raw_mihomo_yaml = fs::read_to_string(path)?;
    let raw_value: YamlValue = serde_yaml::from_str(&raw_mihomo_yaml)?;
    let mut merged = raw_value.clone();
    let patch = override_config.to_yaml_patch()?;
    merge_yaml_values(&mut merged, patch)?;

    if raw_value == merged {
        return Ok(false);
    }

    let serialized = serde_yaml::to_string(&merged)?;
    fs::write(path, serialized)?;
    Ok(true)
}

fn merge_yaml_values(target: &mut YamlValue, patch: YamlValue) -> Result<()> {
    match (target, patch) {
        (YamlValue::Mapping(target), YamlValue::Mapping(patch)) => {
            for (key, patch_value) in patch {
                if let Some(target_value) = target.get_mut(&key) {
                    if matches!(target_value, YamlValue::Mapping(_))
                        && matches!(patch_value, YamlValue::Mapping(_))
                    {
                        merge_yaml_values(target_value, patch_value)?;
                    } else {
                        *target_value = patch_value;
                    }
                } else {
                    target.insert(key, patch_value);
                }
            }
            Ok(())
        }
        (_, _) => bail!("Mihomo config root must be a YAML mapping"),
    }
}

fn toml_to_yaml_value(value: &toml::Value, path: &[String]) -> YamlValue {
    match value {
        toml::Value::String(value) => YamlValue::String(value.clone()),
        toml::Value::Integer(value) => YamlValue::Number((*value).into()),
        toml::Value::Float(value) => {
            serde_yaml::to_value(value).expect("finite TOML float should serialize to YAML")
        }
        toml::Value::Boolean(value) => YamlValue::Bool(*value),
        toml::Value::Datetime(value) => YamlValue::String(value.to_string()),
        toml::Value::Array(values) => YamlValue::Sequence(
            values
                .iter()
                .map(|value| toml_to_yaml_value(value, path))
                .collect(),
        ),
        toml::Value::Table(values) => {
            let mut mapping = Mapping::new();
            for (key, value) in values {
                let output_key = if preserve_map_key(path) {
                    key.clone()
                } else {
                    yaml_key(key, path)
                };
                let mut child_path = path.to_vec();
                child_path.push(key.clone());
                mapping.insert(
                    YamlValue::String(output_key),
                    toml_to_yaml_value(value, &child_path),
                );
            }
            YamlValue::Mapping(mapping)
        }
    }
}

fn yaml_key(key: &str, path: &[String]) -> String {
    if path == ["tls".to_string()] && key == "custom_certifactes" {
        return "custom-certifactes".to_string();
    }
    key.replace('_', "-")
}

fn preserve_map_key(path: &[String]) -> bool {
    if path.first().is_some_and(|value| value == "extra") {
        return true;
    }
    match path {
        [section] => section == "hosts",
        [section, map] => {
            (section == "dns"
                && (map == "nameserver_policy" || map == "proxy_server_nameserver_policy"))
                || (section == "sniffer" && map == "sniff")
                || (section == "tuic_server" && map == "users")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::fs;

    #[test]
    fn sparse_toml_patch_uses_mihomo_yaml_keys() {
        let config = MihomoConfig {
            tun: Some(TunConfig {
                enable: Some(true),
                auto_route: Some(true),
                dns_hijack: Some(vec!["any:53".into()]),
                ..TunConfig::default()
            }),
            ..MihomoConfig::default()
        };

        let patch = config.to_yaml_patch().unwrap();
        assert_eq!(patch["tun"]["enable"], true);
        assert_eq!(patch["tun"]["auto-route"], true);
        assert_eq!(patch["tun"]["dns-hijack"][0], "any:53");
        assert!(!patch["tun"]
            .as_mapping()
            .unwrap()
            .contains_key("auto_route"));
    }

    #[test]
    fn full_local_fixture_maps_nested_sections_and_listener_fields() {
        let source = r#"
port = 7890
tproxy_port = 7893
mode = "rule"
external_controller_tls = "/etc/mihomo/controller.pem"

[tun]
enable = true
stack = "mixed"
auto_route = true
dns_hijack = ["any:53"]
route_address = ["198.18.0.0/16"]

[dns]
enable = true
enhanced_mode = "fake-ip"
nameserver = ["https://dns.example/dns-query"]

[dns.nameserver_policy]
"geosite:cn" = ["https://dns.example/cn"]

[sniffer]
enable = true

[sniffer.sniff.tls]
ports = ["443", "8443"]
override_destination = true

[tuic_server]
enable = true
max_idle_time = 300

[tuic_server.users]
alice = "password"

[[listeners]]
name = "dns-in"
type = "redir"
port = 53
listen = "127.0.0.1"
max_connections = 128
ws_path = "/proxy"
"#;
        let config: MihomoConfig = toml::from_str(source).unwrap();
        let patch = config.to_yaml_patch().unwrap();

        assert_eq!(patch["port"], 7890);
        assert_eq!(patch["tproxy-port"], 7893);
        assert_eq!(
            patch["external-controller-tls"],
            "/etc/mihomo/controller.pem"
        );
        assert_eq!(patch["tun"]["stack"], "mixed");
        assert_eq!(patch["tun"]["route-address"][0], "198.18.0.0/16");
        assert_eq!(patch["dns"]["enhanced-mode"], "fake-ip");
        assert_eq!(
            patch["dns"]["nameserver-policy"]["geosite:cn"][0],
            "https://dns.example/cn"
        );
        assert_eq!(
            patch["sniffer"]["sniff"]["tls"]["override-destination"],
            true
        );
        assert_eq!(patch["tuic-server"]["max-idle-time"], 300);
        assert_eq!(patch["tuic-server"]["users"]["alice"], "password");
        assert_eq!(patch["listeners"][0]["type"], "redir");
        assert_eq!(patch["listeners"][0]["max-connections"], 128);
        assert_eq!(patch["listeners"][0]["ws-path"], "/proxy");
    }

    #[test]
    fn every_supported_top_level_field_has_a_yaml_key_contract() {
        let source = r#"
port = 1
socks_port = 2
mixed_port = 3
redir_port = 4
tproxy_port = 5
ss_config = "ss.json"
vmess_config = "vmess.json"
inbound_tfo = true
inbound_mptcp = true
authentication = ["user:pass"]
skip_auth_prefixes = ["10.0.0.0/8"]
lan_allowed_ips = ["192.168.0.0/16"]
lan_disallowed_ips = ["192.168.1.0/24"]
allow_lan = true
bind_address = "0.0.0.0"
mode = "global"
unified_delay = true
log_level = "debug"
ipv6 = true
external_controller = "127.0.0.1:9090"
external_controller_routing_mark = 10
external_controller_pipe = "/run/mihomo.pipe"
external_controller_unix = "/run/mihomo.sock"
external_controller_tls = "/etc/mihomo/tls.pem"
external_ui = "ui"
external_ui_url = "https://ui.example/"
external_ui_name = "custom-ui"
external_doh_server = "https://doh.example/dns-query"
secret = "secret"
interface_name = "eth0"
routing_mark = 20
geo_auto_update = true
geo_update_interval = 12
geodata_mode = true
geodata_loader = "standard"
geosite_matcher = "hybrid"
global_client_fingerprint = "chrome"
global_ua = "mihomo-test"
etag_support = true
tcp_concurrent = true
find_process_mode = "always"
keep_alive_idle = 30
keep_alive_interval = 10
disable_keep_alive = false

[external_controller_cors]
allow_origins = ["http://127.0.0.1:8080"]
allow_private_network = true

[geox_url]
geoip = "https://geo.example/geoip.dat"
geosite = "https://geo.example/geosite.dat"
mmdb = "https://geo.example/country.mmdb"
asn = "https://geo.example/GeoLite2-ASN.mmdb"

[tls]
certificate = "server.crt"
private_key = "server.key"
client_auth_type = "request"
client_auth_cert = "client.crt"
ech_key = "ech"
custom_certificates = ["ca.pem"]

[experimental]
fingerprints = ["chrome"]
quic_go_disable_gso = true
quic_go_disable_ecn = true
dialer_ip4p_convert = true

[hosts]
"local.example" = "127.0.0.1"

[profile]
store_selected = true
store_fake_ip = true

[tun]
enable = true
stack = "mixed"

[sniffer]
enable = true

[dns]
enable = true

[ntp]
enable = true

[iptables]
enable = true

[tuic_server]
enable = true

[clash_for_android]
append_system_dns = true

[[listeners]]
name = "in"
type = "mixed"
port = 7890
"#;
        let config: MihomoConfig = toml::from_str(source).unwrap();
        let patch = config.to_yaml_patch().unwrap();
        let mapping = patch.as_mapping().unwrap();
        let expected = [
            "port",
            "socks-port",
            "mixed-port",
            "redir-port",
            "tproxy-port",
            "ss-config",
            "vmess-config",
            "inbound-tfo",
            "inbound-mptcp",
            "authentication",
            "skip-auth-prefixes",
            "lan-allowed-ips",
            "lan-disallowed-ips",
            "allow-lan",
            "bind-address",
            "mode",
            "unified-delay",
            "log-level",
            "ipv6",
            "external-controller",
            "external-controller-routing-mark",
            "external-controller-pipe",
            "external-controller-unix",
            "external-controller-tls",
            "external-controller-cors",
            "external-ui",
            "external-ui-url",
            "external-ui-name",
            "external-doh-server",
            "secret",
            "interface-name",
            "routing-mark",
            "geo-auto-update",
            "geo-update-interval",
            "geodata-mode",
            "geodata-loader",
            "geosite-matcher",
            "global-client-fingerprint",
            "global-ua",
            "etag-support",
            "tcp-concurrent",
            "find-process-mode",
            "keep-alive-idle",
            "keep-alive-interval",
            "disable-keep-alive",
            "geox-url",
            "tls",
            "experimental",
            "hosts",
            "profile",
            "tun",
            "sniffer",
            "dns",
            "ntp",
            "iptables",
            "tuic-server",
            "clash-for-android",
            "listeners",
        ];
        assert_eq!(mapping.len(), expected.len());
        for key in expected {
            let yaml_key = YamlValue::String(key.into());
            assert!(mapping.contains_key(&yaml_key), "missing `{key}`");
        }
        assert_eq!(patch["tls"]["custom-certifactes"][0], "ca.pem");
        assert_eq!(
            patch["geox-url"]["asn"],
            "https://geo.example/GeoLite2-ASN.mmdb"
        );
        assert_eq!(
            patch["external-controller-cors"]["allow-origins"][0],
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn tun_fixture_covers_every_supported_tun_key() {
        let source = r#"
[tun]
enable = true
device = "mihoto-tun0"
stack = "gvisor"
dns_hijack = ["any:53"]
auto_route = true
auto_detect_interface = true
mtu = 9000
gso = true
gso_max_size = 65536
inet6_address = ["fd00::1/126"]
iproute2_table_index = 2022
iproute2_rule_index = 9000
auto_redirect = true
auto_redirect_input_mark = 1
auto_redirect_output_mark = 2
auto_redirect_iproute2_fallback_rule_index = 3
loopback_address = ["198.18.0.1"]
strict_route = true
route_address = ["0.0.0.0/0"]
route_address_set = ["geoip-cn"]
route_exclude_address = ["10.0.0.0/8"]
route_exclude_address_set = ["private"]
include_interface = ["eth0"]
exclude_interface = ["docker0"]
include_uid = [1000]
include_uid_range = ["1000-2000"]
exclude_uid = [65534]
exclude_uid_range = ["3000-4000"]
exclude_src_port = [22]
exclude_src_port_range = ["8000-8010"]
exclude_dst_port = [53]
exclude_dst_port_range = ["443-444"]
include_android_user = [0]
include_package = ["com.example.app"]
exclude_package = ["com.example.system"]
include_mac_address = ["00:11:22:33:44:55"]
exclude_mac_address = ["66:77:88:99:aa:bb"]
endpoint_independent_nat = true
udp_timeout = 60
icmp_timeout = 10
disable_icmp_forwarding = true
file_descriptor = 4
inet4_route_address = ["198.18.0.0/16"]
inet6_route_address = ["fd00::/8"]
inet4_route_exclude_address = ["192.168.0.0/16"]
inet6_route_exclude_address = ["fc00::/7"]
recvmsgx = true
sendmsgx = true
"#;
        let config: MihomoConfig = toml::from_str(source).unwrap();
        let patch = config.to_yaml_patch().unwrap();
        let tun = patch["tun"].as_mapping().unwrap();
        let expected = [
            "enable",
            "device",
            "stack",
            "dns-hijack",
            "auto-route",
            "auto-detect-interface",
            "mtu",
            "gso",
            "gso-max-size",
            "inet6-address",
            "iproute2-table-index",
            "iproute2-rule-index",
            "auto-redirect",
            "auto-redirect-input-mark",
            "auto-redirect-output-mark",
            "auto-redirect-iproute2-fallback-rule-index",
            "loopback-address",
            "strict-route",
            "route-address",
            "route-address-set",
            "route-exclude-address",
            "route-exclude-address-set",
            "include-interface",
            "exclude-interface",
            "include-uid",
            "include-uid-range",
            "exclude-uid",
            "exclude-uid-range",
            "exclude-src-port",
            "exclude-src-port-range",
            "exclude-dst-port",
            "exclude-dst-port-range",
            "include-android-user",
            "include-package",
            "exclude-package",
            "include-mac-address",
            "exclude-mac-address",
            "endpoint-independent-nat",
            "udp-timeout",
            "icmp-timeout",
            "disable-icmp-forwarding",
            "file-descriptor",
            "inet4-route-address",
            "inet6-route-address",
            "inet4-route-exclude-address",
            "inet6-route-exclude-address",
            "recvmsgx",
            "sendmsgx",
        ];
        assert_eq!(tun.len(), expected.len());
        for key in expected {
            let yaml_key = YamlValue::String(key.into());
            assert!(tun.contains_key(&yaml_key), "missing TUN key `{key}`");
        }
        assert_eq!(patch["tun"]["stack"], "gvisor");
        assert_eq!(patch["tun"]["dns-hijack"][0], "any:53");
    }

    #[test]
    fn dns_fixture_covers_every_supported_dns_key_and_policy_map() {
        let source = r#"
[dns]
enable = true
prefer_h3 = true
ipv6 = true
ipv6_timeout = 200
use_hosts = true
use_system_hosts = true
respect_rules = true
nameserver = ["https://dns.example/query"]
fallback = ["tls://1.1.1.1:853"]
fallback_lazy_query = true
listen = "127.0.0.1:1053"
listen_routing_mark = 10
enhanced_mode = "redir-host"
fake_ip_range = "198.18.0.1/16"
fake_ip_range6 = "fc00::/18"
fake_ip_filter = ["+.lan"]
fake_ip_filter_mode = "whitelist"
fake_ip_ttl = 60
default_nameserver = ["1.1.1.1"]
cache_algorithm = "lru"
cache_max_size = 1000
proxy_server_nameserver = ["https://dns.example/proxy"]
direct_nameserver = ["https://dns.example/direct"]
direct_nameserver_follow_policy = true

[dns.fallback_filter]
geoip = true
geoip_code = "CN"
ipcidr = ["10.0.0.0/8"]
domain = ["+.example.com"]
geosite = ["gfw"]

[dns.nameserver_policy]
"geosite:cn" = ["https://cn.example/query"]

[dns.proxy_server_nameserver_policy]
"geosite:geolocation-!cn" = "https://foreign.example/query"
"#;
        let config: MihomoConfig = toml::from_str(source).unwrap();
        let patch = config.to_yaml_patch().unwrap();
        let dns = patch["dns"].as_mapping().unwrap();
        let expected = [
            "enable",
            "prefer-h3",
            "ipv6",
            "ipv6-timeout",
            "use-hosts",
            "use-system-hosts",
            "respect-rules",
            "nameserver",
            "fallback",
            "fallback-filter",
            "fallback-lazy-query",
            "listen",
            "listen-routing-mark",
            "enhanced-mode",
            "fake-ip-range",
            "fake-ip-range6",
            "fake-ip-filter",
            "fake-ip-filter-mode",
            "fake-ip-ttl",
            "default-nameserver",
            "cache-algorithm",
            "cache-max-size",
            "nameserver-policy",
            "proxy-server-nameserver",
            "proxy-server-nameserver-policy",
            "direct-nameserver",
            "direct-nameserver-follow-policy",
        ];
        assert_eq!(dns.len(), expected.len());
        for key in expected {
            let yaml_key = YamlValue::String(key.into());
            assert!(dns.contains_key(&yaml_key), "missing DNS key `{key}`");
        }
        assert_eq!(patch["dns"]["fallback-filter"]["geoip-code"], "CN");
        assert_eq!(
            patch["dns"]["nameserver-policy"]["geosite:cn"][0],
            "https://cn.example/query"
        );
        assert_eq!(
            patch["dns"]["proxy-server-nameserver-policy"]["geosite:geolocation-!cn"],
            "https://foreign.example/query"
        );
    }

    #[test]
    fn remaining_section_fixtures_cover_sniffer_ntp_iptables_tuic_and_android() {
        let source = r#"
[sniffer]
enable = true
override_destination = true
sniffing = ["tls", "http"]
force_domain = ["+.example.com"]
skip_src_address = ["10.0.0.0/8"]
skip_dst_address = ["192.168.0.0/16"]
skip_domain = ["+.internal"]
port_whitelist = ["80", "443"]
force_dns_mapping = true
parse_pure_ip = true

[sniffer.sniff.http]
ports = ["80"]
override_destination = false

[ntp]
enable = true
server = "time.example"
port = 123
interval = 3600
dialer_proxy = "DIRECT"
write_to_system = true

[iptables]
enable = true
inbound_interface = "mihoto0"
bypass = ["10.0.0.0/8"]
dns_redirect = true

[tuic_server]
enable = true
listen = "0.0.0.0:443"
token = ["token"]
certificate = "server.crt"
private_key = "server.key"
congestion_controller = "bbr"
max_idle_time = 300
authentication_timeout = 10
alpn = ["h3"]
max_udp_relay_packet_size = 1500
cwnd = 32

[tuic_server.users]
alice = "password"

[clash_for_android]
append_system_dns = true
ui_subtitle_pattern = "mihoto"
"#;
        let config: MihomoConfig = toml::from_str(source).unwrap();
        let patch = config.to_yaml_patch().unwrap();

        let checks = [
            (
                "sniffer",
                [
                    "enable",
                    "override-destination",
                    "sniffing",
                    "force-domain",
                    "skip-src-address",
                    "skip-dst-address",
                    "skip-domain",
                    "port-whitelist",
                    "force-dns-mapping",
                    "parse-pure-ip",
                    "sniff",
                ]
                .as_slice(),
            ),
            (
                "ntp",
                [
                    "enable",
                    "server",
                    "port",
                    "interval",
                    "dialer-proxy",
                    "write-to-system",
                ]
                .as_slice(),
            ),
            (
                "iptables",
                ["enable", "inbound-interface", "bypass", "dns-redirect"].as_slice(),
            ),
            (
                "tuic-server",
                [
                    "enable",
                    "listen",
                    "token",
                    "users",
                    "certificate",
                    "private-key",
                    "congestion-controller",
                    "max-idle-time",
                    "authentication-timeout",
                    "alpn",
                    "max-udp-relay-packet-size",
                    "cwnd",
                ]
                .as_slice(),
            ),
            (
                "clash-for-android",
                ["append-system-dns", "ui-subtitle-pattern"].as_slice(),
            ),
        ];
        for (section, expected) in checks {
            let section_value = patch[section].as_mapping().unwrap();
            assert_eq!(
                section_value.len(),
                expected.len(),
                "wrong `{section}` field count"
            );
            for key in expected {
                let yaml_key = YamlValue::String((*key).into());
                assert!(
                    section_value.contains_key(&yaml_key),
                    "missing `{section}.{key}`"
                );
            }
        }
        assert_eq!(patch["sniffer"]["sniff"]["http"]["ports"][0], "80");
        assert_eq!(patch["tuic-server"]["users"]["alice"], "password");
        assert_eq!(patch["clash-for-android"]["ui-subtitle-pattern"], "mihoto");
    }

    #[test]
    fn apply_is_idempotent_and_rejects_invalid_yaml_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let original = "mode: rule\nfuture-field: keep\n";
        fs::write(&path, original).unwrap();
        let config = MihomoConfig {
            mode: Some(MihomoMode::Direct),
            ..MihomoConfig::default()
        };

        assert!(apply_mihomo_override(path.to_str().unwrap(), &config).unwrap());
        let first = fs::read_to_string(&path).unwrap();
        assert!(!apply_mihomo_override(path.to_str().unwrap(), &config).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), first);

        fs::write(&path, "mode: [broken\n").unwrap();
        assert!(apply_mihomo_override(path.to_str().unwrap(), &config).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "mode: [broken\n");

        fs::write(&path, "not-a-map\n").unwrap();
        assert!(apply_mihomo_override(path.to_str().unwrap(), &config).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "not-a-map\n");
    }

    #[test]
    fn extra_supports_toml_scalar_variants_without_interpolation() {
        let config: MihomoConfig = toml::from_str(
            r#"
[extra]
float-value = 3.125
datetime-value = 2026-08-04T00:00:00Z
"#,
        )
        .unwrap();
        let patch = config.to_yaml_patch().unwrap();
        assert_eq!(patch["float-value"].as_f64(), Some(3.125));
        assert_eq!(patch["datetime-value"], "2026-08-04T00:00:00Z");
    }

    #[test]
    fn invalid_local_types_and_unknown_fields_are_rejected() {
        for source in [
            "not_a_mihomo_field = true",
            "port = \"7890\"",
            "port = null",
            "[tun]\nmtu = -1",
            "[dns]\nenable = \"yes\"",
        ] {
            assert!(
                toml::from_str::<MihomoConfig>(source).is_err(),
                "invalid local configuration was accepted: {source}"
            );
        }
        let dynamic_extra: MihomoConfig = toml::from_str("[extra]\nrules = []\n").unwrap();
        assert!(dynamic_extra.to_yaml_patch().is_err());
    }

    #[test]
    fn nested_maps_merge_and_arrays_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(
			&path,
			"tun:\n  enable: false\n  stack: system\n  route-address:\n    - 10.0.0.0/8\ndns:\n  enable: true\n  nameserver:\n    - 1.1.1.1\nproxies:\n  - name: keep\nrules:\n  - MATCH,DIRECT\n",
		)
		.unwrap();

        let config = MihomoConfig {
            tun: Some(TunConfig {
                enable: Some(true),
                ..TunConfig::default()
            }),
            dns: Some(DnsConfig {
                nameserver: Some(vec!["8.8.8.8".into()]),
                ..DnsConfig::default()
            }),
            ..MihomoConfig::default()
        };

        apply_mihomo_override(path.to_str().unwrap(), &config).unwrap();
        let value: YamlValue = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["tun"]["enable"], true);
        assert_eq!(value["tun"]["stack"], "system");
        assert_eq!(value["tun"]["route-address"][0], "10.0.0.0/8");
        assert_eq!(value["dns"]["enable"], true);
        assert_eq!(value["dns"]["nameserver"][0], "8.8.8.8");
        assert_eq!(value["proxies"][0]["name"], "keep");
        assert_eq!(value["rules"][0], "MATCH,DIRECT");
    }

    #[test]
    fn every_dynamic_collection_survives_a_local_non_dynamic_overlay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut remote = Mapping::new();
        for key in DYNAMIC_TOP_LEVEL_KEYS {
            remote.insert(
                YamlValue::String((*key).into()),
                YamlValue::Sequence(vec![YamlValue::String(format!("{key}-remote"))]),
            );
        }
        remote.insert(
            YamlValue::String("future-field".into()),
            YamlValue::String("keep".into()),
        );
        fs::write(&path, serde_yaml::to_string(&remote).unwrap()).unwrap();

        let config = MihomoConfig {
            mode: Some(MihomoMode::Rule),
            ..MihomoConfig::default()
        };
        assert!(apply_mihomo_override(path.to_str().unwrap(), &config).unwrap());
        let value: YamlValue = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        for key in DYNAMIC_TOP_LEVEL_KEYS {
            let expected = format!("{key}-remote");
            assert_eq!(value[*key][0].as_str(), Some(expected.as_str()));
        }
        assert_eq!(value["future-field"], "keep");
        assert_eq!(value["mode"], "rule");
    }

    #[test]
    fn empty_arrays_replace_remote_arrays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, "authentication:\n  - user:pass\n").unwrap();

        let config = MihomoConfig {
            authentication: Some(Vec::new()),
            ..MihomoConfig::default()
        };
        apply_mihomo_override(path.to_str().unwrap(), &config).unwrap();

        let value: YamlValue = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["authentication"], YamlValue::Sequence(Vec::new()));
    }

    proptest! {
        #[test]
        fn generated_scalar_overlays_preserve_unrelated_remote_values(
            remote_enable in any::<bool>(),
            local_enable in any::<bool>(),
            remote_marker in "[a-z]{1,16}"
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.yaml");
            let remote = format!(
                "tun:\n  enable: {remote_enable}\n  stack: system\n  future-field: {remote_marker:?}\nproxies:\n  - name: keep\n"
            );
            fs::write(&path, remote).unwrap();

            let config = MihomoConfig {
                tun: Some(TunConfig {
                    enable: Some(local_enable),
                    ..TunConfig::default()
                }),
                ..MihomoConfig::default()
            };
            apply_mihomo_override(path.to_str().unwrap(), &config).unwrap();
            let value: YamlValue = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();

            prop_assert_eq!(value["tun"]["enable"].as_bool(), Some(local_enable));
            prop_assert_eq!(value["tun"]["stack"].as_str(), Some("system"));
            prop_assert_eq!(value["tun"]["future-field"].as_str(), Some(remote_marker.as_str()));
            prop_assert_eq!(value["proxies"][0]["name"].as_str(), Some("keep"));
        }

        #[test]
        fn generated_arrays_replace_remote_arrays_but_keep_dynamic_collections(
            remote_authentication in prop::collection::vec("[a-z]{1,12}", 0..8),
            local_authentication in prop::collection::vec("[a-z]{1,12}", 0..8)
        ) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.yaml");
            let remote = serde_yaml::to_string(&serde_yaml::Mapping::from_iter([
                (YamlValue::String("authentication".into()), serde_yaml::to_value(&remote_authentication).unwrap()),
                (YamlValue::String("proxy-groups".into()), serde_yaml::to_value(vec!["keep-group"]).unwrap()),
            ])).unwrap();
            fs::write(&path, remote).unwrap();

            let config = MihomoConfig {
                authentication: Some(local_authentication.clone()),
                ..MihomoConfig::default()
            };
            apply_mihomo_override(path.to_str().unwrap(), &config).unwrap();
            let value: YamlValue = serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();

            prop_assert_eq!(&value["authentication"], &serde_yaml::to_value(&local_authentication).unwrap());
            prop_assert_eq!(value["proxy-groups"][0].as_str(), Some("keep-group"));
        }
    }

    #[test]
    fn extra_preserves_exact_keys_and_rejects_dynamic_keys() {
        let config: MihomoConfig = toml::from_str(
            r#"
[extra]
"external-controller-cors" = { allow-origins = ["http://localhost"] }
"#,
        )
        .unwrap();
        let patch = config.to_yaml_patch().unwrap();
        assert_eq!(
            patch["external-controller-cors"]["allow-origins"][0],
            "http://localhost"
        );

        for key in DYNAMIC_TOP_LEVEL_KEYS {
            let dynamic = MihomoConfig {
                extra: toml::map::Map::from_iter([(
                    (*key).to_string(),
                    toml::Value::Array(Vec::new()),
                )]),
                ..MihomoConfig::default()
            };
            assert!(
                dynamic.to_yaml_patch().is_err(),
                "dynamic key `{key}` was accepted"
            );
        }
    }

    #[test]
    fn environment_syntax_is_not_interpolated() {
        let config: MihomoConfig = toml::from_str(
            "external_controller = '${MIHOMO_CONTROLLER}'\n\n[extra]\n\"x-local\" = '${VALUE}'\n",
        )
        .unwrap();
        let patch = config.to_yaml_patch().unwrap();
        assert_eq!(patch["external-controller"], "${MIHOMO_CONTROLLER}");
        assert_eq!(patch["x-local"], "${VALUE}");
    }

    #[test]
    fn extra_cannot_duplicate_typed_fields() {
        let config = MihomoConfig {
            external_controller: Some("127.0.0.1:9090".into()),
            extra: toml::map::Map::from_iter([(
                "external-controller".into(),
                toml::Value::String("x".into()),
            )]),
            ..MihomoConfig::default()
        };
        assert!(config.to_yaml_patch().is_err());
    }

    #[test]
    fn dynamic_fields_cannot_be_declared_as_typed_toml_fields() {
        for key in DYNAMIC_TOP_LEVEL_KEYS {
            let source = format!("{key} = []");
            assert!(
                toml::from_str::<MihomoConfig>(&source).is_err(),
                "dynamic field `{key}` was accepted outside extra"
            );
        }
    }
}
